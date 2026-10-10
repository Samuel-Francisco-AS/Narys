//! Local C3 gates traverse the real Worker graph, Scheduler and SQLite ledger.
use super::*;
use crate::{
    agents::planner::{PlanCapability, PlanStepV1, PlanV1},
    cognition::{
        allocation_policy,
        policy::{self, CognitiveTargetPolicy, RoutingMode, ThinkingLevel},
        provider::{Provider, ProviderFuture},
        registry::ProviderRegistry,
        scheduler::Scheduler,
        telemetry::{Provenance, QuotaDimension, QuotaScope},
        types::*,
    },
    cognitive_resources::*,
    persistence::checkpoints::*,
};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicUsize, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Call {
    pub(super) unit: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) effort: Option<ThinkingLevel>,
    pub(super) dependencies: Option<String>,
}
struct Mock {
    id: &'static str,
    calls: Arc<Mutex<Vec<Call>>>,
    variants: bool,
    partial_failure: bool,
    hold_after_output: bool,
    rendezvous: Option<Arc<tokio::sync::Barrier>>,
    entered: Arc<tokio::sync::Notify>,
}
impl Provider for Mock {
    fn supports_invocation(&self, c: &ProviderInvocationConfig, m: &InvocationMode) -> bool {
        c.valid()
            && m.text_stream()
            && (!self.variants
                || matches!(
                    (c.model.as_str(), c.thinking_level),
                    ("model-x", Some(ThinkingLevel::Low)) | ("model-y", Some(ThinkingLevel::High))
                ))
    }
    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if cancelled.load(Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            let unit = request
                .input
                .split("SUBTAREFA ")
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_owned();
            self.calls.lock().unwrap().push(Call {
                unit: unit.clone(),
                provider: self.id.into(),
                model: request.target.invocation.model.clone(),
                effort: request.target.invocation.thinking_level,
                dependencies: request
                    .input
                    .split("RESULTADOS DAS DEPENDÊNCIAS:\n")
                    .nth(1)
                    .map(str::to_owned),
            });
            self.entered.notify_one();
            if let Some(barrier) = &self.rendezvous {
                barrier.wait().await;
            }
            let text = format!("verified-result-{unit}");
            chunk(ProviderChunk { text: text.clone() })?;
            if self.hold_after_output {
                std::future::pending::<()>().await;
            }
            if self.partial_failure {
                return Err(ProviderError::Unavailable {
                    retry_after_ms: None,
                });
            }
            Ok(ProviderResponse {
                text,
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: Some(2),
                    thought_tokens: None,
                    output_tokens_measured: true,
                },
            })
        })
    }
}
pub(super) struct Fixture {
    pub(super) directory: PathBuf,
    pub(super) db: Database,
    pub(super) runtime: Arc<ProviderRuntime>,
    pub(super) snapshot: TaskPolicySnapshot,
    pub(super) calls: Arc<Mutex<Vec<Call>>>,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) context: Option<ContextBundle>,
    pub(super) entered: Arc<tokio::sync::Notify>,
}
impl Fixture {
    pub(super) fn new(
        mode: RoutingMode,
        variants: bool,
        partial_failure: bool,
        parallel: bool,
    ) -> Self {
        Self::new_economic(mode, variants, partial_failure, parallel, false, false)
    }
    pub(super) fn new_economic(
        mode: RoutingMode,
        variants: bool,
        partial_failure: bool,
        parallel: bool,
        paid: bool,
        allow: bool,
    ) -> Self {
        Self::new_recovery_fixture(
            mode,
            variants,
            partial_failure,
            parallel,
            paid,
            allow,
            true,
            false,
        )
    }
    pub(super) fn new_recovery_fixture(
        mode: RoutingMode,
        variants: bool,
        partial_failure: bool,
        parallel: bool,
        paid: bool,
        allow: bool,
        known_cost: bool,
        partial_b: bool,
    ) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let directory = std::env::temp_dir().join(format!(
            "c3-graph-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let db = Database::for_test(directory.join("test.sqlite3"));
        crate::cognition::task_graph_runtime_tests::seed_identity(&db);
        let mut conn = db.open().unwrap();
        let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
        routing.routing_mode = mode;
        routing.max_provider_calls = 16;
        routing.retry_enabled = true;
        routing.max_retries = 1;
        routing.retry_backoff_ms = 0;
        routing.targets = vec![CognitiveTargetPolicy {
            provider_id: "runtime-a".into(),
            model: if variants { "model-x" } else { "model-a" }.into(),
            thinking_level: if variants {
                Some(ThinkingLevel::Low)
            } else {
                None
            },
        }];
        if mode != RoutingMode::Fixed {
            routing.targets.push(CognitiveTargetPolicy {
                provider_id: "runtime-b".into(),
                model: "model-b".into(),
                thinking_level: None,
            });
        }
        let mut dto = allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
        if allow {
            dto.paid_use_policy = allocation_policy::PaidUseMode::AllowKnownCostWithinBudget;
            dto.max_paid_currency = Some("USD".into());
            dto.max_paid_micros = Some(100);
        }
        allocation_policy::save_role_settings(&mut conn, &routing, &dto).unwrap();
        let captured =
            allocation_policy::load_role_runtime_policy(&conn, CognitiveRole::Worker).unwrap();
        let snapshot =
            TaskPolicySnapshot::new(captured.routing, captured.allocation_snapshot).unwrap();
        let context = ContextBuilder::build(
            &conn,
            ContextRequest {
                domain: None,
                kind: None,
                min_importance: 0,
                memory_limit: 0,
                include_recent_conversation: false,
            },
        )
        .unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let rendezvous = parallel.then(|| Arc::new(tokio::sync::Barrier::new(2)));
        let mut registry = ProviderRegistry::default();
        let mut catalog = ResourceCatalog::default();
        for (id, priority) in [("runtime-a", 0), ("runtime-b", 1), ("runtime-c", 2)] {
            let config = ProviderConfig {
                id: id.into(),
                enabled: true,
                priority,
                capabilities: ProviderCapabilities::text_stream(),
            };
            registry
                .register(
                    config.clone(),
                    Arc::new(Mock {
                        id,
                        calls: calls.clone(),
                        variants: variants && id == "runtime-a",
                        partial_failure: (partial_failure && id == "runtime-a")
                            || (partial_b && id == "runtime-b"),
                        hold_after_output: partial_b && id == "runtime-b",
                        rendezvous: rendezvous.clone(),
                        entered: entered.clone(),
                    }),
                )
                .unwrap();
            let mut resource = CognitiveResource::from_provider_config(
                ResourceIdentity {
                    id: ResourceId::new(id).unwrap(),
                    class: ResourceClass::CognitiveProvider,
                    family: ProviderFamily::new(id).unwrap(),
                    access_path: AccessPath::new(if id == "runtime-b" {
                        "path-b"
                    } else {
                        "path-a"
                    })
                    .unwrap(),
                    billing_domain: BillingDomain {
                        id: BillingDomainId::new(id).unwrap(),
                    },
                },
                &config,
            )
            .unwrap();
            if variants && id == "runtime-a" {
                let profiles = [("model-x", "low"), ("model-y", "high")]
                    .into_iter()
                    .map(|(id, effort)| {
                        let mut model = ModelProfile::unknown(ModelId::new(id).unwrap());
                        model.supported_efforts = known(vec![EffortProfile {
                            id: EffortId::new(effort).unwrap(),
                            availability: CatalogFact::Unknown,
                            facts: ExecutionFacts::default(),
                        }]);
                        model
                    })
                    .collect();
                resource.models = known(profiles);
            }
            if paid && id == "runtime-b" {
                resource.economics.billing_kind = known(BillingKind::MeteredBilling);
                let mut profile = ModelProfile::unknown(ModelId::new("model-b").unwrap());
                if known_cost {
                    profile.facts.execution.monetary_cost =
                        known(MonetaryAmount::new("USD", 10).unwrap());
                }
                resource.models = known(vec![profile]);
            }
            catalog.register(resource).unwrap();
        }
        let scheduler =
            Scheduler::new(registry).with_auto_allocator(ProviderAutoAllocator::new(catalog));
        Self {
            directory,
            db,
            runtime: Arc::new(ProviderRuntime {
                scheduler: Arc::new(scheduler),
            }),
            snapshot,
            calls,
            cancelled: Arc::new(AtomicBool::new(false)),
            context: Some(context),
            entered,
        }
    }
    pub(super) async fn run(
        &mut self,
        steps: Vec<PlanStepV1>,
        action: impl Fn(&serde_json::Value) + Send + Sync + 'static,
    ) -> (ExecutionOutcome, Vec<serde_json::Value>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let saved = events.clone();
        let channel = Channel::new(move |body| {
            if let crate::channel::InvokeResponseBody::Json(json) = body {
                let event = serde_json::from_str::<serde_json::Value>(&json).unwrap();
                action(&event);
                saved.lock().unwrap().push(event);
            }
            Ok(())
        });
        let timeouts = self
            .snapshot
            .routing()
            .targets
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
            .collect();
        let planner = OrchestratorResult {
            provider_id: "planner-runtime".into(),
            usage: SchedulerUsage::default(),
            plan: plan(steps),
        };
        let started_at = now();
        let mut outcome = execute_workers(
            self.db.clone(),
            self.runtime.clone(),
            TaskId(100),
            self.cancelled.clone(),
            &channel,
            &AtomicU32::new(0),
            self.snapshot.clone(),
            timeouts,
            self.context.take().unwrap(),
            planner,
        )
        .await;
        let _ = finalize_execution(
            &self.db,
            TaskId(100),
            &self.cancelled,
            &mut outcome,
            started_at,
        )
        .await;
        let events = events.lock().unwrap().clone();
        (outcome, events)
    }
    pub(super) fn receipts(&self) -> Vec<CheckpointRecord> {
        let conn = self.db.open().unwrap();
        let bindings: Vec<(u64, String)> = conn.prepare("SELECT unit_sequence,source_key FROM main.cognitive_checkpoints WHERE root_task_id=100 ORDER BY unit_sequence").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        bindings
            .into_iter()
            .map(|(seq, source)| {
                match CheckpointRepository::lookup(
                    &conn,
                    ExecutionUnitId::new(100, seq).unwrap(),
                    &ExecutionSource::subtask(source).unwrap(),
                )
                .unwrap()
                {
                    CheckpointLoadResult::Committed(record) => record,
                    other => panic!("invalid receipt: {other:?}"),
                }
            })
            .collect()
    }
    pub(super) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn known<T>(value: T) -> CatalogFact<T> {
    CatalogFact::known(value, CatalogProvenance::IntegrationCatalog, Some(42)).unwrap()
}
pub(super) fn step(id: &str, deps: &[&str]) -> PlanStepV1 {
    PlanStepV1 {
        id: id.into(),
        description: "deterministic cognitive work".into(),
        required_capabilities: vec![PlanCapability::Planning],
        depends_on: deps.iter().map(|s| (*s).into()).collect(),
    }
}
pub(super) fn plan(steps: Vec<PlanStepV1>) -> PlanV1 {
    PlanV1 {
        version: 1,
        objective: "local gate".into(),
        steps,
        risks: vec![],
        needs_user_input: false,
        questions: vec![],
    }
}
pub(super) fn sequential() -> Vec<PlanStepV1> {
    vec![step("a", &[]), step("b", &["a"])]
}
pub(super) fn exhaust(s: &Scheduler, id: &str, scope: QuotaScope) {
    s.telemetry.observe_quota(
        id,
        scope,
        QuotaDimension::RequestsPerMinute,
        Some(10),
        Some(0),
        None,
        Provenance::ProviderHeader,
    );
}

#[tokio::test]
async fn c3_b_c_n_auto_changes_resource_and_access_path_only_after_a_is_committed() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, events) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_output_observed" && e["subtask_id"] == "a" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(
        f.calls()
            .iter()
            .map(|c| (c.unit.as_str(), c.provider.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "runtime-a"), ("b", "runtime-b")]
    );
    let receipts = f.receipts();
    assert_eq!(
        receipts[0].checkpoint().allocation().resource_id.as_str(),
        "runtime-a"
    );
    assert_eq!(
        receipts[0].checkpoint().allocation().access_path.as_str(),
        "path-a"
    );
    assert_eq!(
        receipts[1].checkpoint().allocation().resource_id.as_str(),
        "runtime-b"
    );
    assert_eq!(
        receipts[1].checkpoint().context().completed_dependencies(),
        &[receipts[0].checkpoint().id()]
    );
    let committed = events
        .iter()
        .position(|e| e["type"] == "subtask_completed" && e["subtask_id"] == "a")
        .unwrap();
    let selected = events
        .iter()
        .position(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert!(committed < selected);
    assert_eq!(
        events[selected]["transitions"][0]["change"],
        serde_json::json!({"resource":true,"accessPath":true,"model":true,"effort":false})
    );
    assert!(matches!(receipts[0].replay(), ReplayDecision::Forbidden(_)));
}

#[tokio::test]
async fn c3_d_cross_model_and_effort_on_same_resource_and_access_path() {
    let mut f = Fixture::new(RoutingMode::Auto, true, false, false);
    // Keep the second authorized runtime out of this model-transition gate.
    exhaust(&f.runtime.scheduler, "runtime-b", QuotaScope::Provider);
    let s = f.runtime.scheduler.clone();
    let (outcome, events) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(
                    &s,
                    "runtime-a",
                    QuotaScope::Model {
                        model: "model-x".into(),
                    },
                );
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let calls = f.calls();
    assert_eq!(
        (calls[0].model.as_str(), calls[0].effort),
        ("model-x", Some(ThinkingLevel::Low))
    );
    assert_eq!(
        (calls[1].model.as_str(), calls[1].effort),
        ("model-y", Some(ThinkingLevel::High))
    );
    assert_eq!(calls[0].provider, calls[1].provider);
    let selected = events
        .iter()
        .find(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert_eq!(
        selected["transitions"][0]["change"],
        serde_json::json!({"resource":false,"accessPath":false,"model":true,"effort":true})
    );
    let receipts = f.receipts();
    assert_eq!(
        receipts[0].checkpoint().allocation().model_id.as_str(),
        "model-x"
    );
    assert_eq!(
        receipts[1].checkpoint().allocation().model_id.as_str(),
        "model-y"
    );
    assert_eq!(
        receipts[0]
            .checkpoint()
            .allocation()
            .effort
            .as_ref()
            .unwrap()
            .as_str(),
        "low"
    );
    assert_eq!(
        receipts[1]
            .checkpoint()
            .allocation()
            .effort
            .as_ref()
            .unwrap()
            .as_str(),
        "high"
    );
}

#[tokio::test]
async fn c3_e_same_winner_is_unchanged_not_a_false_handoff_change() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(f.calls()[0].provider, f.calls()[1].provider);
    let e = events
        .iter()
        .find(|e| e["type"] == "subtask_started" && e["subtask_id"] == "b")
        .unwrap();
    assert_eq!(
        e["transitions"][0]["change"],
        serde_json::json!({"resource":false,"accessPath":false,"model":false,"effort":false})
    );
}

#[tokio::test]
async fn c3_f_policy_is_captured_once_settings_cannot_expand_the_task_universe() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                let mut conn = db.open().unwrap();
                let mut routing = policy::load(&conn, CognitiveRole::Worker).unwrap();
                routing.targets[0].provider_id = "runtime-c".into();
                let mut dto = allocation_policy::load(&conn, CognitiveRole::Worker).unwrap();
                dto.allocation_profile = AllocationProfile::Fast;
                allocation_policy::save_role_settings(&mut conn, &routing, &dto).unwrap();
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(f.calls()[1].provider, "runtime-b");
    for record in f.receipts() {
        assert_eq!(record.checkpoint().policy(), &f.snapshot);
    }
    let new =
        allocation_policy::load_role_runtime_policy(&f.db.open().unwrap(), CognitiveRole::Worker)
            .unwrap();
    assert_ne!(new.routing, *f.snapshot.routing());
    assert_ne!(new.allocation_snapshot.as_ref(), f.snapshot.allocation());
}

#[tokio::test]
async fn c3_g_checkpoint_uses_actual_pin_when_winner_differs_from_config_first_and_later_rank() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    exhaust(&f.runtime.scheduler, "runtime-a", QuotaScope::Provider);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(vec![step("a", &[])], move |e| {
            if e["type"] == "subtask_output_observed" {
                s.telemetry.invalidate_provider_quotas("runtime-a");
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(f.calls()[0].provider, "runtime-b");
    let cp = f.receipts().remove(0);
    assert_eq!(
        cp.checkpoint().provenance().runtime_id.as_str(),
        "runtime-b"
    );
    assert_eq!(cp.checkpoint().allocation().model_id.as_str(), "model-b");
    assert_eq!(cp.checkpoint().allocation().access_path.as_str(), "path-b");
    let targets = f
        .snapshot
        .routing()
        .provider_targets(
            &f.snapshot
                .routing()
                .targets
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
                .collect(),
        )
        .unwrap();
    assert_eq!(
        f.runtime
            .scheduler
            .ranked_provider_allocations(
                &ProviderSelection::Auto,
                &targets,
                Some(&f.snapshot.allocation().unwrap().to_runtime().unwrap())
            )
            .unwrap()[0]
            .target()
            .provider_id,
        "runtime-a"
    );
}

#[tokio::test]
async fn c3_h_cancellation_after_committed_receipt_wins_before_successor_request() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let cancelled = f.cancelled.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                cancelled.store(true, Ordering::Release);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    assert_eq!(outcome.error_code, Some("cancelled"));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.receipts().len(), 1);
    assert_eq!(
        outcome.graph.unwrap().state("a"),
        Some(SubtaskState::Completed)
    );
}

#[tokio::test]
async fn c3_h_cancellation_in_successor_started_callback_is_rechecked_by_scheduler() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let cancelled = f.cancelled.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_started" && e["subtask_id"] == "b" {
                cancelled.store(true, Ordering::Release);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Cancelled);
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.receipts().len(), 1);
}

#[tokio::test]
async fn c3_i_failed_checkpoint_write_blocks_dependents_and_has_local_error() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, events) = f.run(sequential(), move |e| {
        if e["type"] == "subtask_output_observed" && e["subtask_id"] == "a" { db.open().unwrap().execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON cognitive_checkpoints BEGIN SELECT RAISE(ABORT,'local failure'); END;").unwrap(); }
    }).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("handoff_checkpoint_write_failed"));
    assert_eq!(f.calls().len(), 1);
    assert!(f.receipts().is_empty());
    assert!(!events.iter().any(|e| e["type"] == "subtask_completed"));
    let conn = f.db.open().unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM checkpoint_task_policies", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn c3_j_unknown_effect_in_durable_predecessor_blocks_successor() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, _) = f.run(sequential(), move |e| {
        if e["type"] == "subtask_completed" && e["subtask_id"] == "a" { db.open().unwrap().execute_batch("UPDATE main.cognitive_checkpoints SET effect_state='unknown_or_in_flight',checkpoint_json=json_set(checkpoint_json,'$.effects','unknown_or_in_flight') WHERE source_key='a';").unwrap(); }
    }).await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert_eq!(outcome.error_code, Some("continuation_history_invalid"));
    assert_eq!(f.calls().len(), 1);
    assert_paused_without_terminal_history(&f, &outcome);
    let receipt = f.receipts().remove(0);
    assert_eq!(
        receipt.checkpoint().effects(),
        EffectState::UnknownOrInFlight
    );
    assert_eq!(receipt.boundary(), HandoffBoundary::Unknown);
}

#[tokio::test]
async fn c3_k_partial_output_failure_never_recreates_a_or_creates_boundary() {
    let mut f = Fixture::new(RoutingMode::Auto, false, true, false);
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(f.calls().len(), 1);
    assert!(f.receipts().is_empty());
    assert!(events
        .iter()
        .any(|e| e["type"] == "subtask_output_observed"));
    assert!(!events
        .iter()
        .any(|e| e["type"] == "subtask_retry" || e["type"] == "provider_fallback"));
}

#[tokio::test]
async fn c3_l_fixed_target_is_preserved_when_scarcity_changes() {
    let mut f = Fixture::new(RoutingMode::Fixed, false, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                s.telemetry.observe_quota(
                    "runtime-a",
                    QuotaScope::Provider,
                    QuotaDimension::RequestsPerMinute,
                    Some(100),
                    Some(1),
                    None,
                    Provenance::ProviderHeader,
                );
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert!(f.calls().iter().all(|c| c.provider == "runtime-a"));
    assert!(f
        .receipts()
        .iter()
        .all(|r| r.checkpoint().allocation().access_path.as_str() == "provider_runtime"));
}

#[tokio::test]
async fn c3_m_preferred_order_and_d3_distribution_ignore_economic_scarcity() {
    let mut f = Fixture::new(RoutingMode::Preferred, false, false, false);
    f.runtime.scheduler.telemetry.observe_quota(
        "runtime-a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        Some(100),
        Some(1),
        None,
        Provenance::ProviderHeader,
    );
    let (outcome, _) = f
        .run(
            vec![step("a", &[]), step("b", &["a"]), step("c", &["b"])],
            |_| {},
        )
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    assert_eq!(
        f.calls()
            .iter()
            .map(|c| c.provider.as_str())
            .collect::<Vec<_>>(),
        vec!["runtime-a", "runtime-b", "runtime-a"]
    );
}

#[tokio::test]
async fn c3_o_parallel_units_overlap_with_separate_pins_and_receipts() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, true);
    let s = f.runtime.scheduler.clone();
    let (outcome, events) = f
        .run(vec![step("a", &[]), step("b", &[])], move |e| {
            if e["type"] == "subtask_output_observed" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let mut calls = f.calls();
    calls.sort_by(|a, b| a.unit.cmp(&b.unit));
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.unit.as_str(), c.provider.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "runtime-a"), ("b", "runtime-b")]
    );
    let receipts = f.receipts();
    assert_eq!(receipts.len(), 2);
    assert!(receipts
        .iter()
        .all(|r| r.checkpoint().context().completed_dependencies().is_empty()));
    assert!(events
        .iter()
        .filter(|e| e["type"] == "subtask_started")
        .all(|e| e["transitions"] == serde_json::json!([])));
}

#[tokio::test]
async fn c3_p_corrupt_or_changed_receipt_cannot_release_a_dependent() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, _) = f.run(sequential(), move |e| {
        if e["type"] == "subtask_completed" && e["subtask_id"] == "a" { db.open().unwrap().execute_batch("UPDATE main.cognitive_checkpoints SET checkpoint_json='{}' WHERE source_key='a';").unwrap(); }
    }).await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert_eq!(outcome.error_code, Some("continuation_checkpoint_invalid"));
    assert_eq!(f.calls().len(), 1);
    assert_paused_without_terminal_history(&f, &outcome);
}

#[tokio::test]
async fn c3_p_dependency_links_are_exact_with_multi_parent_and_out_of_order_plan() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (outcome, _) = f
        .run(
            vec![step("c", &["a", "b"]), step("b", &[]), step("a", &[])],
            |_| {},
        )
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let receipts = f.receipts();
    assert_eq!(receipts.len(), 3);
    let cp = receipts[2].checkpoint();
    assert_eq!(
        cp.context().completed_dependencies(),
        &[receipts[0].checkpoint().id(), receipts[1].checkpoint().id()]
    );
    assert!(cp.id().unit_id().sequence() > receipts[1].checkpoint().id().unit_id().sequence());
}

#[tokio::test]
async fn c3_q_events_and_ledger_reconstruct_allocations_without_cognitive_content() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (outcome, events) = f.run(sequential(), |_| {}).await;
    assert_eq!(outcome.state, TaskState::Completed);
    let receipts = f.receipts();
    for e in events.iter().filter(|e| e["type"] == "subtask_started") {
        let seq = e["unit_id"]["sequence"].as_u64().unwrap() as usize;
        assert_eq!(
            e["allocation"],
            serde_json::to_value(receipts[seq - 1].checkpoint().allocation()).unwrap()
        );
    }
    for e in events.iter().filter(|e| e["type"] == "subtask_completed") {
        let seq = e["checkpoint_id"]["unitId"]["sequence"].as_u64().unwrap() as usize;
        assert_eq!(
            e["checkpoint_id"],
            serde_json::to_value(receipts[seq - 1].checkpoint().id()).unwrap()
        );
    }
    let conn = f.db.open().unwrap();
    let json: Vec<String> = conn
        .prepare("SELECT checkpoint_json FROM cognitive_checkpoints")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for value in json
        .into_iter()
        .chain(events.into_iter().map(|e| e.to_string()))
    {
        assert!(!value.contains("verified-result-"));
        assert!(!value.contains("deterministic cognitive work"));
    }
}

#[tokio::test]
async fn c3_r_every_unit_has_one_execution_one_receipt_and_never_replays_a() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let (outcome, _) = f
        .run(
            vec![step("a", &[]), step("b", &["a"]), step("c", &["a", "b"])],
            |_| {},
        )
        .await;
    assert_eq!(outcome.state, TaskState::Completed);
    let calls = f.calls();
    assert_eq!(calls.len(), 3);
    let ids: std::collections::BTreeSet<_> = calls.iter().map(|c| &c.unit).collect();
    assert_eq!(ids.len(), 3);
    assert!(f
        .receipts()
        .iter()
        .all(|r| matches!(r.replay(), ReplayDecision::Forbidden(_))));
}

#[tokio::test]
async fn c3_r_prepared_unit_cannot_be_allocated_twice_or_given_another_policy_target_list() {
    let f = Fixture::new(RoutingMode::Auto, false, false, false);
    let graph = TaskGraph::compile(&plan(sequential())).unwrap();
    let timeouts = f
        .snapshot
        .routing()
        .targets
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
        .collect();
    let mut targets = f.snapshot.routing().provider_targets(&timeouts).unwrap();
    let mut bridge = TaskGraphHandoff::new(100, f.snapshot.clone()).unwrap();
    let pin = bridge
        .prepare(
            &f.db,
            &graph,
            "a",
            &f.runtime.scheduler,
            &targets,
            0,
            &f.cancelled,
        )
        .await
        .unwrap();
    assert_eq!(
        bridge
            .prepare(
                &f.db,
                &graph,
                "a",
                &f.runtime.scheduler,
                &targets,
                0,
                &f.cancelled
            )
            .await
            .unwrap_err(),
        "handoff_unit_already_allocated"
    );
    targets[0].provider_id = "runtime-c".into();
    let mut another = TaskGraphHandoff::new(101, f.snapshot.clone()).unwrap();
    assert_eq!(
        another
            .prepare(
                &f.db,
                &graph,
                "a",
                &f.runtime.scheduler,
                &targets,
                0,
                &f.cancelled
            )
            .await
            .unwrap_err(),
        "handoff_targets_mismatch"
    );
    assert_eq!(pin.pin().target().provider_id, "runtime-a");
    assert!(f.calls().is_empty());
}

#[tokio::test]
async fn c3_p_valid_but_different_checkpoint_sequence_does_not_match_receipt() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let db = f.db.clone();
    let (outcome, _) = f.run(sequential(), move |e| {
        if e["type"] == "subtask_completed" && e["subtask_id"] == "a" { db.open().unwrap().execute_batch("UPDATE main.cognitive_checkpoints SET checkpoint_sequence=2,checkpoint_json=json_set(checkpoint_json,'$.id.sequence',2) WHERE source_key='a';").unwrap(); }
    }).await;
    assert_eq!(outcome.state, TaskState::Paused);
    assert_eq!(outcome.error_code, Some("continuation_checkpoint_mismatch"));
    assert_eq!(f.calls().len(), 1);
    assert_paused_without_terminal_history(&f, &outcome);
    assert_eq!(f.receipts()[0].checkpoint().id().sequence(), 2);
}

#[tokio::test]
async fn c3_allocation_unavailable_is_not_provider_failed_or_a_new_execution() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, false);
    let s = f.runtime.scheduler.clone();
    let (outcome, _) = f
        .run(sequential(), move |e| {
            if e["type"] == "subtask_completed" && e["subtask_id"] == "a" {
                exhaust(&s, "runtime-a", QuotaScope::Provider);
                exhaust(&s, "runtime-b", QuotaScope::Provider);
            }
        })
        .await;
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(outcome.error_code, Some("handoff_allocation_unavailable"));
    assert_eq!(f.calls().len(), 1);
    assert_eq!(f.receipts().len(), 1);
}

#[tokio::test]
async fn c3_o_reallocating_b_while_a_runs_never_changes_a_pin_or_serializes_them() {
    let mut f = Fixture::new(RoutingMode::Auto, false, false, true);
    let mut graph = TaskGraph::compile(&plan(vec![step("a", &[]), step("b", &[])])).unwrap();
    let timeouts = f
        .snapshot
        .routing()
        .targets
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
        .collect();
    let targets = f.snapshot.routing().provider_targets(&timeouts).unwrap();
    let mut bridge = TaskGraphHandoff::new(100, f.snapshot.clone()).unwrap();
    let a = bridge
        .prepare(
            &f.db,
            &graph,
            "a",
            &f.runtime.scheduler,
            &targets,
            0,
            &f.cancelled,
        )
        .await
        .unwrap();
    let original = a.pin().variant().clone();
    let context = Arc::new(f.context.take().unwrap());
    let context_a = context.clone();
    let scheduler = f.runtime.scheduler.clone();
    let cancelled = f.cancelled.clone();
    let a_call = tokio::spawn(async move {
        run_worker(
            scheduler,
            TaskId(100),
            "a".into(),
            a,
            "SUBTAREFA a:".into(),
            None,
            Some(512),
            2,
            RetryPolicy {
                enabled: true,
                max_retries: 1,
                initial_backoff_ms: 0,
            },
            context_a,
            &cancelled,
            &Channel::new(|_| Ok(())),
            &AtomicU32::new(0),
        )
        .await
        .1
        .unwrap()
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), f.entered.notified())
        .await
        .unwrap();
    graph.mark_running("a").unwrap();
    exhaust(&f.runtime.scheduler, "runtime-a", QuotaScope::Provider);
    let b = bridge
        .prepare(
            &f.db,
            &graph,
            "b",
            &f.runtime.scheduler,
            &targets,
            0,
            &f.cancelled,
        )
        .await
        .unwrap();
    assert_eq!(b.pin().target().provider_id, "runtime-b");
    let result_b = run_worker(
        f.runtime.scheduler.clone(),
        TaskId(100),
        "b".into(),
        b,
        "SUBTAREFA b:".into(),
        None,
        Some(512),
        2,
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 0,
        },
        context,
        &f.cancelled,
        &Channel::new(|_| Ok(())),
        &AtomicU32::new(0),
    )
    .await
    .1
    .unwrap();
    let result_a = a_call.await.unwrap();
    let a_receipt = bridge
        .commit_completed(&f.db, "a", &result_a)
        .await
        .unwrap();
    let b_receipt = bridge
        .commit_completed(&f.db, "b", &result_b)
        .await
        .unwrap();
    assert_eq!(a_receipt.checkpoint().allocation(), &original);
    assert_eq!(
        b_receipt.checkpoint().allocation().resource_id.as_str(),
        "runtime-b"
    );
    assert_eq!(f.calls().len(), 2);
    assert_eq!(
        bridge
            .commit_completed(&f.db, "a", &result_a)
            .await
            .unwrap_err(),
        "handoff_unit_already_committed"
    );
}

fn assert_paused_without_terminal_history(f: &Fixture, outcome: &ExecutionOutcome) {
    assert_eq!(
        outcome.graph.as_ref().unwrap().state("b"),
        Some(SubtaskState::Blocked)
    );
    let conn = f.db.open().unwrap();
    let stored: (String, Option<String>, u32, u32) = conn.query_row(
        "SELECT state,pause_reason,(SELECT COUNT(*) FROM task_records WHERE task_id=100),(SELECT COUNT(*) FROM task_subtask_records WHERE root_task_id=100) FROM cognitive_continuations WHERE root_task_id=100",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).unwrap();
    assert_eq!(
        stored,
        ("paused".into(), Some("recovery_required".into()), 0, 0)
    );
}
