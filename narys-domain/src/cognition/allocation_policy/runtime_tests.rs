//! Final B4 gate through actual role request builders, B3/B1/B2 and Scheduler.
use super::*;
use crate::cognition::{
    admission::TrafficClass,
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::{Fact, InvocationObservation, UsageDimension},
    types::*,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

struct Fake {
    seen: Mutex<Vec<ProviderTarget>>,
    errors: Mutex<VecDeque<ProviderError>>,
    supports: AtomicUsize,
    endpoint: Option<String>,
}
impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(vec![]),
            errors: Mutex::new(VecDeque::new()),
            supports: AtomicUsize::new(0),
            endpoint: None,
        })
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
        invocation.valid() && mode.valid()
    }
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { panic!("must use observed boundary") })
    }
    fn execute_observed<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
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
            if let Some(error) = self.errors.lock().unwrap().pop_front() {
                return Err(error);
            }
            let usage = ProviderUsage {
                calls: 1,
                output_tokens: 1,
                output_tokens_measured: true,
                total_tokens: Some(1),
                ..Default::default()
            };
            observation.final_usage(usage);
            Ok(ProviderResponse {
                text: "synthetic".into(),
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
        capabilities: ProviderCapabilities::with_structured_output(),
    }
}
fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, None).unwrap()
}
fn resource(id: &str, cost: u16, latency: u64, tier: Option<u16>) -> CognitiveResource {
    let mut r = CognitiveResource::from_provider_config(
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
    .unwrap();
    let mut m = ModelProfile::unknown(ModelId::new("configured").unwrap());
    m.facts.execution.relative_cost = known(RelativeCostTier::new(cost).unwrap());
    m.facts.execution.latency_ms = known(latency);
    m.facts.execution.cognitive_tier = tier.map_or(CatalogFact::Unknown, |n| {
        known(CognitiveTier::new(n).unwrap())
    });
    r.models = known(vec![m]);
    r
}
fn paid(r: &mut CognitiveResource, cost: Option<(&str, u64)>, kind: BillingKind) {
    r.economics.billing_kind = known(kind);
    if let Some((currency, micros)) = cost {
        if let CatalogFact::Known { value: models, .. } = &mut r.models {
            models[0].facts.execution.monetary_cost =
                known(MonetaryAmount::new(currency, micros).unwrap());
        }
    }
}
fn reserve(r: &mut CognitiveResource, remaining: u64) {
    let id = AllowanceDimensionId::new("local-window").unwrap();
    r.economics.allowances.push(AllowanceState {
        id: id.clone(),
        unit: AllowanceUnit::Requests,
        limit: known(100),
        remaining: known(remaining),
        reset: CatalogFact::Unknown,
    });
    if let CatalogFact::Known { value: models, .. } = &mut r.models {
        models[0]
            .facts
            .execution
            .allowance_costs
            .push(AllowanceConsumption {
                dimension_id: id,
                unit: AllowanceUnit::Requests,
                amount: known(1),
            });
    }
}
fn scheduler(resources: Vec<CognitiveResource>, providers: &[(&str, Arc<Fake>)]) -> Scheduler {
    let mut registry = ProviderRegistry::default();
    for (id, fake) in providers {
        registry.register(config(id), fake.clone()).unwrap();
    }
    let mut catalog = ResourceCatalog::default();
    for r in resources {
        catalog.register(r).unwrap();
    }
    Scheduler::new(registry).with_auto_allocator(ProviderAutoAllocator::new(catalog))
}
fn fresh() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::persistence::migrations::apply(&conn).unwrap();
    conn
}
fn routing(
    conn: &Connection,
    role: CognitiveRole,
    ids: &[&str],
    mode: RoutingMode,
) -> CognitiveRolePolicy {
    let mut p = policy::load(conn, role).unwrap();
    p.routing_mode = mode;
    p.max_provider_calls = 4;
    p.targets = ids
        .iter()
        .map(|id| super::super::policy::CognitiveTargetPolicy {
            provider_id: id.to_string(),
            model: "configured".into(),
            thinking_level: None,
        })
        .collect();
    p
}
fn timeouts(p: &CognitiveRolePolicy) -> HashMap<String, ProviderTimeouts> {
    p.targets
        .iter()
        .map(|t| {
            (
                t.provider_id.clone(),
                ProviderTimeouts {
                    request_timeout_ms: 1000,
                    stream_idle_timeout_ms: 1000,
                },
            )
        })
        .collect()
}
fn request(snapshot: RoleRuntimePolicy) -> ProviderTaskRequest {
    let p = snapshot.routing;
    let t = timeouts(&p);
    match p.role {
        CognitiveRole::Conversation => {
            crate::luna::runtime::chat_budget_and_request(
                42,
                "private-prompt".into(),
                vec![],
                crate::cognition::orchestrator::technical_context(),
                &p,
                &t,
                snapshot.allocation,
            )
            .unwrap()
            .1
        }
        CognitiveRole::Summary => {
            crate::cognition::summary::summary_request(&[], false, &p, t, snapshot.allocation)
        }
        CognitiveRole::Orchestrator => crate::cognition::orchestrator::task_graph_request(
            "private-objective",
            &p,
            t,
            snapshot.allocation,
        ),
        CognitiveRole::Worker => ProviderTaskRequest {
            allocation_policy: snapshot.allocation,
            selection: p.selection(),
            targets: p.provider_targets(&t).unwrap(),
            traffic_class: TrafficClass::ForegroundTask,
            mode: InvocationMode::default(),
            input: "private-unit".into(),
            internal_system_instruction: None,
            history: vec![],
            context: Arc::new(crate::cognition::orchestrator::technical_context()),
            max_output_tokens: None,
            affinity_key: None,
            estimated_context_bytes: 0,
            required_capabilities: ProviderCapabilities::text_stream(),
        },
    }
}
async fn run(
    s: &Scheduler,
    req: ProviderTaskRequest,
) -> (Result<TaskResult, SchedulerError>, Vec<SchedulerEvent>) {
    let mut events = vec![];
    let result = s
        .run_with_retry(
            req,
            TaskBudget {
                max_provider_calls: 4,
                max_output_tokens: None,
            },
            RetryPolicy {
                enabled: false,
                max_retries: 0,
                initial_backoff_ms: 0,
            },
            &AtomicBool::new(false),
            &mut |e| {
                events.push(e);
                Ok(())
            },
        )
        .await;
    (result, events)
}
fn zero_side_effects(s: &Scheduler, id: &str, fake: &Fake) {
    assert_eq!(fake.calls(), 0);
    let rate = s
        .rate
        .read_only_snapshots()
        .into_iter()
        .find(|r| r.provider_id == id)
        .unwrap();
    assert_eq!(rate.pending_reservations, 0);
    assert!(rate
        .constraints
        .iter()
        .all(|c| c.reserved == 0 && c.consumed == 0));
    assert_eq!(
        s.admission_snapshot()
            .into_iter()
            .find(|r| r.provider_id == id)
            .unwrap()
            .total_admissions,
        0
    );
    let telemetry = s
        .telemetry_snapshot()
        .into_iter()
        .find(|r| r.provider_id == id)
        .unwrap();
    assert!(matches!(
        telemetry.usage[&UsageDimension::Requests].observed,
        Fact::Known { value: 0, .. }
    ));
}
#[tokio::test]
async fn b4_auto_none_before_any_event_reservation_admission_or_provider() {
    let a = Fake::new();
    let b = Fake::new();
    let s = scheduler(
        vec![resource("a", 0, 0, None), resource("b", 0, 0, None)],
        &[("a", a.clone()), ("b", b.clone())],
    );
    let conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b"],
        RoutingMode::Auto,
    );
    let req = request(RoleRuntimePolicy {
        routing: p.clone(),
        allocation: None,
        allocation_snapshot: None,
    });
    let (result, events) = run(&s, req).await;
    assert_eq!(result.unwrap_err(), SchedulerError::InvalidTargetConfig);
    assert!(events.is_empty());
    assert_eq!(
        s.ranked_provider_ids(
            &p.selection(),
            &p.provider_targets(&timeouts(&p)).unwrap(),
            &ProviderCapabilities::text_stream(),
            &InvocationMode::default(),
            None
        )
        .unwrap_err(),
        SchedulerError::InvalidTargetConfig
    );
    for (id, fake) in [("a", a), ("b", b)] {
        zero_side_effects(&s, id, &fake);
        assert_eq!(fake.supports.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn b4_fixed_preferred_no_economics_even_with_missing_table() {
    let mut conn = fresh();
    conn.execute_batch("DROP TABLE cognitive_role_allocation_policies")
        .unwrap();
    let a = Fake::new();
    let b = Fake::new();
    let mut ra = resource("a", 255, 10000, None);
    paid(&mut ra, None, BillingKind::MeteredBilling);
    reserve(&mut ra, 0);
    let s = scheduler(
        vec![ra, resource("b", 0, 0, None)],
        &[("a", a.clone()), ("b", b.clone())],
    );
    for (mode, ids) in [
        (RoutingMode::Fixed, vec!["a"]),
        (RoutingMode::Preferred, vec!["a", "b"]),
    ] {
        let explicit = routing(&conn, CognitiveRole::Conversation, &ids, mode);
        policy::save(&mut conn, &explicit).unwrap();
        let snapshot = load_role_runtime_policy(&conn, CognitiveRole::Conversation).unwrap();
        assert!(snapshot.allocation.is_none());
        let (result, events) = run(&s, request(snapshot)).await;
        assert_eq!(result.unwrap().provider_id, "a");
        assert!(events
            .iter()
            .any(|e| matches!(e, SchedulerEvent::Selected { score: None, .. })));
    }
    assert_eq!(a.calls(), 2);
    zero_side_effects(&s, "b", &b);
}
#[tokio::test]
async fn b4_persisted_profile_changes_winner_new_task_and_old_snapshot_stays_immutable() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.allocation_profile = AllocationProfile::Economy;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    let old = load_role_runtime_policy(&conn, p.role).unwrap();
    let frozen = old.clone();
    let a = Fake::new();
    let b = Fake::new();
    let s = scheduler(
        vec![
            resource("a", 0, 3000, Some(2)),
            resource("b", 20, 0, Some(4)),
        ],
        &[("a", a), ("b", b)],
    );
    dto.allocation_profile = AllocationProfile::Fast;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    assert_eq!(run(&s, request(old)).await.0.unwrap().provider_id, "a");
    assert_eq!(
        frozen.allocation.unwrap().policy().profile,
        AllocationProfile::Economy
    );
    assert_eq!(
        run(
            &s,
            request(load_role_runtime_policy(&conn, p.role).unwrap())
        )
        .await
        .0
        .unwrap()
        .provider_id,
        "b"
    );
}
#[tokio::test]
async fn b4_persisted_explicit_auto_expand_only_authorized_providers() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.allocation_profile = AllocationProfile::Economy;
    let mut ra = resource("a", 20, 0, Some(2));
    if let CatalogFact::Known { value: models, .. } = &mut ra.models {
        let mut cheap = models[0].clone();
        cheap.id = ModelId::new("cheap").unwrap();
        cheap.facts.execution.relative_cost = known(RelativeCostTier::new(0).unwrap());
        models.push(cheap);
    }
    let a = Fake::new();
    let b = Fake::new();
    let d = Fake::new();
    let s = scheduler(
        vec![
            ra,
            resource("b", 100, 0, Some(2)),
            resource("d", 0, 0, Some(255)),
        ],
        &[("a", a.clone()), ("b", b), ("d", d.clone())],
    );
    dto.variant_selection_mode = VariantSelectionMode::Explicit;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    run(
        &s,
        request(load_role_runtime_policy(&conn, p.role).unwrap()),
    )
    .await
    .0
    .unwrap();
    assert_eq!(a.seen.lock().unwrap()[0].invocation.model, "configured");
    dto.variant_selection_mode = VariantSelectionMode::Auto;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    run(
        &s,
        request(load_role_runtime_policy(&conn, p.role).unwrap()),
    )
    .await
    .0
    .unwrap();
    assert_eq!(a.seen.lock().unwrap()[1].invocation.model, "cheap");
    zero_side_effects(&s, "d", &d);
}
#[tokio::test]
async fn b4_persisted_quality_floor_filters_insufficient_and_unknown() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b", "c"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.allocation_profile = AllocationProfile::Economy;
    let a = Fake::new();
    let b = Fake::new();
    let c = Fake::new();
    let s = scheduler(
        vec![
            resource("a", 0, 0, Some(2)),
            resource("b", 20, 0, Some(4)),
            resource("c", 0, 0, None),
        ],
        &[("a", a.clone()), ("b", b), ("c", c.clone())],
    );
    save_role_settings(&mut conn, &p, &dto).unwrap();
    assert_eq!(
        run(
            &s,
            request(load_role_runtime_policy(&conn, p.role).unwrap())
        )
        .await
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    dto.minimum_cognitive_tier = Some(4);
    save_role_settings(&mut conn, &p, &dto).unwrap();
    assert_eq!(
        run(
            &s,
            request(load_role_runtime_policy(&conn, p.role).unwrap())
        )
        .await
        .0
        .unwrap()
        .provider_id,
        "b"
    );
    assert_eq!(a.calls(), 1);
    zero_side_effects(&s, "c", &c);
}
#[tokio::test]
async fn b4_persisted_paid_guards_currency_unknown_exceeded_zero_and_prepaid() {
    for (cost, budget, kind, balance, allowed) in [
        (Some(("USD", 5)), 5, BillingKind::MeteredBilling, None, true),
        (None, 5, BillingKind::MeteredBilling, None, false),
        (
            Some(("EUR", 5)),
            5,
            BillingKind::MeteredBilling,
            None,
            false,
        ),
        (
            Some(("USD", 6)),
            5,
            BillingKind::MeteredBilling,
            None,
            false,
        ),
        (
            Some(("USD", 1)),
            0,
            BillingKind::MeteredBilling,
            None,
            false,
        ),
        (Some(("USD", 0)), 0, BillingKind::MeteredBilling, None, true),
        (
            Some(("USD", 0)),
            0,
            BillingKind::PrepaidCredits,
            None,
            false,
        ),
        (
            Some(("USD", 5)),
            5,
            BillingKind::PrepaidCredits,
            Some(("USD", 5)),
            true,
        ),
        (
            Some(("USD", 5)),
            5,
            BillingKind::PrepaidCredits,
            Some(("USD", 4)),
            false,
        ),
        (
            Some(("USD", 5)),
            5,
            BillingKind::PrepaidCredits,
            Some(("EUR", 5)),
            false,
        ),
    ] {
        let mut conn = fresh();
        let p = routing(
            &conn,
            CognitiveRole::Conversation,
            &["a", "b"],
            RoutingMode::Auto,
        );
        let mut dto = load(&conn, p.role).unwrap();
        dto.allocation_profile = AllocationProfile::Fast;
        dto.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
        dto.max_paid_currency = Some("USD".into());
        dto.max_paid_micros = Some(budget);
        save_role_settings(&mut conn, &p, &dto).unwrap();
        let mut ra = resource("a", 0, 0, None);
        paid(&mut ra, cost, kind);
        if let Some((currency, amount)) = balance {
            ra.economics.monetary_balance = known(MonetaryAmount::new(currency, amount).unwrap());
        }
        let a = Fake::new();
        let b = Fake::new();
        let s = scheduler(
            vec![ra, resource("b", 0, 10000, None)],
            &[("a", a.clone()), ("b", b)],
        );
        assert_eq!(
            run(
                &s,
                request(load_role_runtime_policy(&conn, p.role).unwrap())
            )
            .await
            .0
            .unwrap()
            .provider_id,
            if allowed { "a" } else { "b" }
        );
        if !allowed {
            zero_side_effects(&s, "a", &a);
        }
    }
}
#[tokio::test]
async fn b4_persisted_reserve_null_thresholds_do_not_invent_reserve() {
    for profile in [AllocationProfile::Economy, AllocationProfile::Balanced] {
        let mut conn = fresh();
        let p = routing(
            &conn,
            CognitiveRole::Conversation,
            &["a", "b"],
            RoutingMode::Auto,
        );
        let mut dto = load(&conn, p.role).unwrap();
        dto.allocation_profile = profile;
        let mut ra = resource("a", 0, 0, None);
        reserve(&mut ra, 5);
        let mut rb = resource("b", 0, 0, None);
        reserve(&mut rb, 80);
        let s = scheduler(vec![ra, rb], &[("a", Fake::new()), ("b", Fake::new())]);
        save_role_settings(&mut conn, &p, &dto).unwrap();
        assert_eq!(
            run(
                &s,
                request(load_role_runtime_policy(&conn, p.role).unwrap())
            )
            .await
            .0
            .unwrap()
            .provider_id,
            "a"
        );
        dto.reduced_below_percent = Some(40);
        dto.reserve_below_percent = Some(10);
        save_role_settings(&mut conn, &p, &dto).unwrap();
        assert_eq!(
            run(
                &s,
                request(load_role_runtime_policy(&conn, p.role).unwrap())
            )
            .await
            .0
            .unwrap()
            .provider_id,
            "b"
        );
    }
}
fn loopback() -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let join = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![];
        let mut buf = [0; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let len = header
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + len {
                    break;
                }
            }
        }
        counted.fetch_add(1, Ordering::SeqCst);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    (url, hits, join)
}
#[tokio::test]
async fn b4_final_integrated_persisted_deny_zero_paid_http_then_allow_new_task() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.allocation_profile = AllocationProfile::Economy;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    let old = load_role_runtime_policy(&conn, p.role).unwrap();
    let old_copy = old.clone();
    let (url_a, hits_a, join_a) = loopback();
    let (url_b, hits_b, join_b) = loopback();
    let a = Arc::new(Fake {
        endpoint: Some(url_a),
        ..Arc::try_unwrap(Fake::new()).ok().unwrap()
    });
    let b = Arc::new(Fake {
        endpoint: Some(url_b),
        ..Arc::try_unwrap(Fake::new()).ok().unwrap()
    });
    let mut ra = resource("a", 0, 0, None);
    paid(&mut ra, Some(("USD", 5)), BillingKind::MeteredBilling);
    let s = scheduler(
        vec![ra, resource("b", 200, 10000, None)],
        &[("a", a.clone()), ("b", b.clone())],
    );
    // Configure a real LR-8 local request window so zero reservations/debits can be asserted.
    s.rate
        .set_policy(
            "a",
            crate::cognition::rate::RatePolicy {
                limits: vec![crate::cognition::rate::LocalRateLimit {
                    scope: crate::cognition::telemetry::QuotaScope::Provider,
                    dimension: crate::cognition::telemetry::QuotaDimension::RequestsPerMinute,
                    capacity: 10,
                    window: crate::cognition::rate::FixedWindow {
                        period_ms: 60000,
                        anchor_unix_ms: 0,
                    },
                }],
                daily_budget: None,
            },
        )
        .unwrap();
    let (result, events) = run(&s, request(old)).await;
    assert_eq!(result.unwrap().provider_id, "b");
    join_b.join().unwrap();
    assert_eq!(hits_b.load(Ordering::SeqCst), 1);
    assert_eq!(hits_a.load(Ordering::SeqCst), 0);
    zero_side_effects(&s, "a", &a);
    assert!(events.iter().any(|e|matches!(e,SchedulerEvent::Selected {provider_id,routing_reason:"auto_allocator",score:Some(_),..} if provider_id=="b")));
    dto.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
    dto.max_paid_currency = Some("USD".into());
    dto.max_paid_micros = Some(100);
    save_role_settings(&mut conn, &p, &dto).unwrap();
    assert_eq!(
        old_copy.allocation.unwrap().policy().paid_use,
        PaidUsePolicy::Deny
    );
    let (result, events) = run(
        &s,
        request(load_role_runtime_policy(&conn, p.role).unwrap()),
    )
    .await;
    assert_eq!(result.unwrap().provider_id, "a");
    join_a.join().unwrap();
    assert_eq!(hits_a.load(Ordering::SeqCst), 1);
    let public_events: Vec<_> = events
        .iter()
        .filter_map(|event| {
            if let SchedulerEvent::Selected {
                provider_id,
                model,
                attempt,
                routing_reason,
                score,
            } = event
            {
                Some(crate::luna::task::TaskEventKind::ProviderSelected {
                    provider_id: provider_id.clone(),
                    model: model.clone(),
                    attempt: *attempt,
                    routing_reason: routing_reason.to_string(),
                    score: *score,
                })
            } else {
                None
            }
        })
        .collect();
    let json = serde_json::to_string(&public_events).unwrap();
    for forbidden in [
        "USD",
        "maxPaid",
        "budget",
        "minimumCognitive",
        "reserveBelow",
        "private-prompt",
        "private-objective",
        "monetary",
        "ScoreBreakdown",
    ] {
        assert!(!json.contains(forbidden), "{forbidden}");
    }
}
#[tokio::test]
async fn b4_save_during_429_503_keeps_frozen_chain_variants_scores_and_paid_exclusions() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b", "c", "d"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.variant_selection_mode = VariantSelectionMode::Explicit;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    let snapshot = load_role_runtime_policy(&conn, p.role).unwrap();
    let before = snapshot.clone();
    let a = Fake::new();
    a.errors
        .lock()
        .unwrap()
        .push_back(ProviderError::RateLimited {
            retry_after_ms: Some(1),
        });
    let b = Fake::new();
    b.errors
        .lock()
        .unwrap()
        .push_back(ProviderError::Unavailable {
            retry_after_ms: Some(1),
        });
    let c = Fake::new();
    let d = Fake::new();
    let e = Fake::new();
    let mut rd = resource("d", 0, 0, Some(4));
    paid(&mut rd, Some(("USD", 1)), BillingKind::MeteredBilling);
    let s = scheduler(
        vec![
            resource("a", 0, 0, None),
            resource("b", 0, 0, None),
            resource("c", 0, 0, None),
            rd,
            resource("e", 0, 0, Some(4)),
        ],
        &[
            ("a", a.clone()),
            ("b", b.clone()),
            ("c", c.clone()),
            ("d", d.clone()),
            ("e", e.clone()),
        ],
    );
    let mut events = vec![];
    let mut saved = false;
    let result = s
        .run_with_retry(
            request(snapshot),
            TaskBudget {
                max_provider_calls: 4,
                max_output_tokens: None,
            },
            RetryPolicy {
                enabled: false,
                max_retries: 0,
                initial_backoff_ms: 0,
            },
            &AtomicBool::new(false),
            &mut |event| {
                if !saved && matches!(event, SchedulerEvent::Selected { .. }) {
                    let mut next = p.clone();
                    next.targets
                        .push(super::super::policy::CognitiveTargetPolicy {
                            provider_id: "e".into(),
                            model: "configured".into(),
                            thinking_level: None,
                        });
                    dto.allocation_profile = AllocationProfile::Fast;
                    dto.variant_selection_mode = VariantSelectionMode::Auto;
                    dto.minimum_cognitive_tier = Some(4);
                    dto.reduced_below_percent = Some(100);
                    dto.reserve_below_percent = Some(100);
                    dto.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
                    dto.max_paid_currency = Some("USD".into());
                    dto.max_paid_micros = Some(7);
                    save_role_settings(&mut conn, &next, &dto).unwrap();
                    saved = true;
                }
                events.push(event);
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(result.provider_id, "c");
    assert!(saved);
    assert_eq!((a.calls(), b.calls(), c.calls()), (1, 1, 1));
    let selections: Vec<_> = events
        .iter()
        .filter_map(|ev| {
            if let SchedulerEvent::Selected {
                provider_id,
                model,
                score,
                ..
            } = ev
            {
                Some((provider_id.clone(), model.clone(), *score))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        selections,
        vec![
            ("a".into(), "configured".into(), Some(2133)),
            ("b".into(), "configured".into(), Some(2127)),
            ("c".into(), "configured".into(), Some(2121))
        ]
    );
    for (id, fake) in [("d", d), ("e", e)] {
        zero_side_effects(&s, id, &fake);
        assert_eq!(
            fake.supports.load(Ordering::SeqCst),
            if id == "d" { 1 } else { 0 }
        );
    }
    assert_eq!(
        before.allocation.unwrap().policy().paid_use,
        PaidUsePolicy::Deny
    );
    assert_eq!(
        load_role_runtime_policy(&conn, p.role)
            .unwrap()
            .allocation
            .unwrap()
            .policy()
            .profile,
        AllocationProfile::Fast
    );
}
async fn assert_role_wiring(role: CognitiveRole) {
    let mut conn = fresh();
    for configured in [
        CognitiveRole::Conversation,
        CognitiveRole::Summary,
        CognitiveRole::Orchestrator,
        CognitiveRole::Worker,
    ] {
        let p = routing(&conn, configured, &["a", "b"], RoutingMode::Auto);
        let mut dto = load(&conn, configured).unwrap();
        dto.allocation_profile = AllocationProfile::Fast;
        if configured != role {
            dto.paid_use_policy = PaidUseMode::AllowKnownCostWithinBudget;
            dto.max_paid_currency = Some("USD".into());
            dto.max_paid_micros = Some(100);
        }
        save_role_settings(&mut conn, &p, &dto).unwrap();
    }
    let snapshot = load_role_runtime_policy(&conn, role).unwrap();
    let p = snapshot.routing.clone();
    let a = Fake::new();
    let b = Fake::new();
    let mut ra = resource("a", 0, 0, None);
    paid(&mut ra, Some(("USD", 1)), BillingKind::MeteredBilling);
    let s = scheduler(
        vec![ra, resource("b", 0, 10000, None)],
        &[("a", a.clone()), ("b", b.clone())],
    );
    let mut req = request(snapshot);
    if role == CognitiveRole::Worker {
        let ranked = crate::cognition::task_graph_runtime::rank_worker_targets(
            &s,
            &p,
            &req.targets,
            req.allocation_policy.as_ref(),
        )
        .unwrap();
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].provider_id, "b");
        req.targets = vec![ranked[0].clone()];
        req.selection = ProviderSelection::Fixed("b".into());
        req.allocation_policy = None;
        // Fixed execution after the initial ranking must not ask for a policy again.
    }
    let (result, events) = run(&s, req).await;
    assert_eq!(result.unwrap().provider_id, "b");
    zero_side_effects(&s, "a", &a);
    assert_eq!(b.calls(), 1);
    assert_eq!(a.supports.load(Ordering::SeqCst), 1);
    if role != CognitiveRole::Worker {
        assert!(events.iter().any(|e| matches!(
            e,
            SchedulerEvent::Selected {
                routing_reason: "auto_allocator",
                ..
            }
        )));
    }
}
#[tokio::test]
async fn b4_conversation_wiring_reaches_b3() {
    assert_role_wiring(CognitiveRole::Conversation).await;
}
#[tokio::test]
async fn b4_summary_wiring_reaches_b3() {
    assert_role_wiring(CognitiveRole::Summary).await;
}
#[tokio::test]
async fn b4_orchestrator_wiring_reaches_b3() {
    assert_role_wiring(CognitiveRole::Orchestrator).await;
}
#[tokio::test]
async fn b4_worker_wiring_initial_allocation_then_fixed_execution() {
    assert_role_wiring(CognitiveRole::Worker).await;
}
#[test]
fn b4_settings_api_returns_only_local_config_from_same_read_boundary() {
    let mut conn = fresh();
    let p = routing(
        &conn,
        CognitiveRole::Conversation,
        &["a", "b"],
        RoutingMode::Auto,
    );
    let mut dto = load(&conn, p.role).unwrap();
    dto.allocation_profile = AllocationProfile::Economy;
    save_role_settings(&mut conn, &p, &dto).unwrap();
    let settings = load_all_role_settings(&conn).unwrap();
    assert_eq!(settings.len(), 4);
    assert_eq!(settings[0].allocation_policy, dto);
    assert_eq!(settings[0].policy, p);
    let json = serde_json::to_string(&settings).unwrap();
    for marker in [
        "catalog",
        "accountId",
        "observations",
        "ScoreBreakdown",
        "secret",
        "prompt",
    ] {
        assert!(!json.contains(marker));
    }
}
#[test]
fn b4_scheduler_chain_and_resources_have_no_database_policy_authority() {
    let scheduler = include_str!("../scheduler.rs");
    let engine = scheduler
        .split("fn resolve_provider_chain(")
        .nth(1)
        .unwrap()
        .split("pub async fn run(")
        .next()
        .unwrap();
    for dependency in [
        "Database",
        "Connection",
        "rusqlite",
        "CognitiveRole",
        "settings::",
        "policy::load",
    ] {
        assert!(!engine.contains(dependency));
    }
    let bridge = include_str!("../../cognitive_resources/provider_bridge.rs");
    let allocator = bridge
        .split("pub struct ProviderAutoAllocator")
        .nth(1)
        .unwrap()
        .split("impl ProviderAutoAllocator")
        .next()
        .unwrap();
    assert!(allocator.contains("catalog: ResourceCatalog"));
    assert!(!allocator.contains("policy:"));
    assert!(!allocator.contains("floor:"));
    assert!(!bridge.contains("rusqlite"));
}
