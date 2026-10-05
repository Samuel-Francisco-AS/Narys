//! Aggregate helper tests, no IPC hacks, commercial providers or real credentials.
use super::{
    admission::{AdmissionConfig, TrafficClass},
    provider::{Provider, ProviderFuture},
    rate::{
        ClockReading, DailyBudgetPolicy, FixedWindow, LocalRateLimit, RateClock, RatePolicy,
        TokenUpperBound,
    },
    registry::ProviderRegistry,
    resilience::{CircuitState, ResilienceConfig, RuntimeJitter},
    scheduler::Scheduler,
    telemetry::{
        Fact, Provenance, QuotaDimension, QuotaScope, Timing, UsageDimension, MAX_FACT_VALUE,
    },
    types::*,
};
use crate::persistence::database::Database;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

struct Clock(AtomicU64);
impl RateClock for Clock {
    fn now(&self) -> ClockReading {
        let n = self.0.load(Ordering::SeqCst);
        ClockReading {
            monotonic_ms: n,
            unix_ms: Some(1_000),
        }
    }
}
struct NeverCalled {
    calls: Arc<AtomicU64>,
    _private_data: &'static str,
}
impl Provider for NeverCalled {
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { panic!("snapshot must never execute a provider") })
    }
}
fn fixture(db: Option<Database>) -> (Scheduler, Arc<Clock>, Arc<AtomicU64>) {
    let calls = Arc::new(AtomicU64::new(0));
    let clock = Arc::new(Clock(AtomicU64::new(0)));
    let mut registry = ProviderRegistry::default();
    for id in ["gemini", "groq", "cloudflare", "mistral"] {
        registry.register(ProviderConfig { id: id.into(), enabled: id != "mistral", priority: 1, capabilities: ProviderCapabilities::text_stream() }, Arc::new(NeverCalled { calls: calls.clone(), _private_data: "sk-operational-private-marker Bearer private_account_id private_prompt private_output private_reasoning raw_body Authorization unlock_material" })).unwrap();
    }
    let scheduler = Scheduler::with_resilience_config(
        registry,
        AdmissionConfig::default(),
        clock.clone(),
        db,
        ResilienceConfig {
            open_duration_ms: 30,
            ..ResilienceConfig::default()
        },
        Arc::new(RuntimeJitter::default()),
    )
    .unwrap();
    (scheduler, clock, calls)
}
fn daily() -> RatePolicy {
    RatePolicy {
        limits: vec![LocalRateLimit {
            scope: QuotaScope::Model {
                model: "model-a".into(),
            },
            dimension: QuotaDimension::RequestsPerMinute,
            capacity: 7,
            window: FixedWindow {
                period_ms: 100,
                anchor_unix_ms: 1_000,
            },
        }],
        daily_budget: Some(DailyBudgetPolicy {
            anchor_unix_ms: 1_000,
            max_requests: Some(10),
            max_accounted_tokens: Some(100),
        }),
    }
}
#[test]
fn operational_all_registered_ids_align_and_unknown_is_preserved_without_calls() {
    let (s, _, calls) = fixture(None);
    for _ in 0..100 {
        let snap = s.operational_snapshot();
        let ids: Vec<_> = snap.telemetry.iter().map(|p| &p.provider_id).collect();
        assert_eq!(ids, vec!["cloudflare", "gemini", "groq", "mistral"]);
        assert_eq!(
            ids,
            snap.admission
                .iter()
                .map(|p| &p.provider_id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            ids,
            snap.rate.iter().map(|p| &p.provider_id).collect::<Vec<_>>()
        );
        assert_eq!(
            ids,
            snap.resilience
                .iter()
                .map(|p| &p.provider_id)
                .collect::<Vec<_>>()
        );
        for t in snap.telemetry {
            assert!(matches!(
                t.usage[&UsageDimension::Requests].observed,
                Fact::Known { value: 0, .. }
            ));
            assert!(matches!(
                t.usage[&UsageDimension::TotalTokens].observed,
                Fact::Unknown
            ));
            assert!(matches!(t.retry_hint, Fact::Unknown));
            assert!(t.quotas[0].dimensions.values().all(|q| matches!(
                (&q.limit, &q.remaining, &q.reset),
                (Fact::Unknown, Fact::Unknown, Fact::Unknown)
            )));
        }
        assert!(snap
            .admission
            .iter()
            .all(|a| a.total_admissions == 0 && a.total_waited == 0));
        assert!(snap
            .rate
            .iter()
            .all(|r| r.pending_reservations == 0 && r.local_blocks == 0));
        assert!(snap.resilience.iter().all(|r| r.transition_count == 0));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn operational_open_half_open_probe_and_cooldown_are_read_only() {
    let (s, clock, _) = fixture(None);
    for _ in 0..3 {
        s.resilience
            .authorize("groq", 0)
            .unwrap()
            .finish(true, Some(&ProviderError::Timeout));
    }
    let before = serde_json::to_value(s.resilience_snapshot()).unwrap();
    let open = s.operational_snapshot();
    let r = open
        .resilience
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert_eq!(r.circuit_state, CircuitState::Open);
    assert_eq!(r.open_remaining_ms, 30);
    assert_eq!(
        serde_json::to_value(s.resilience_snapshot()).unwrap(),
        before
    );
    clock.0.store(30, Ordering::SeqCst);
    assert_eq!(
        s.operational_snapshot()
            .resilience
            .iter()
            .find(|p| p.provider_id == "groq")
            .unwrap()
            .circuit_state,
        CircuitState::Open
    );
    let probe = s.resilience.authorize("groq", 0).unwrap();
    let snap = s.operational_snapshot();
    let r = snap
        .resilience
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert_eq!(r.circuit_state, CircuitState::HalfOpen);
    assert_eq!(r.half_open_probes_active, 1);
    probe.finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(80),
        }),
    );
    let snap = s.operational_snapshot();
    let r = snap
        .resilience
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert_eq!(r.cooldown_remaining_ms, 80);
    assert_eq!(r.half_open_probes_active, 0);
    assert_eq!(r.transition_count, 2);
}
#[tokio::test]
async fn operational_active_calls_queue_classes_and_cleanup_are_visible() {
    let (s, _, _) = fixture(None);
    let cancel = AtomicBool::new(false);
    let one = s
        .admission
        .acquire(
            "groq",
            TrafficClass::ForegroundInteractive,
            &cancel,
            &mut |_| Ok(()),
        )
        .await
        .unwrap();
    let two = s
        .admission
        .acquire("groq", TrafficClass::ForegroundTask, &cancel, &mut |_| {
            Ok(())
        })
        .await
        .unwrap();
    let (queued_tx, queued_rx) = tokio::sync::oneshot::channel();
    let waiting = async {
        let mut tx = Some(queued_tx);
        s.admission
            .acquire("groq", TrafficClass::Background, &cancel, &mut |_| {
                tx.take().unwrap().send(()).unwrap();
                Ok(())
            })
            .await
    };
    tokio::pin!(waiting);
    tokio::select! { biased;
        result = &mut waiting => panic!("must queue, got {}", result.is_ok()),
        _ = queued_rx => {}
    }
    let before = serde_json::to_value(s.admission_snapshot()).unwrap();
    let snap = s.operational_snapshot();
    let a = snap
        .admission
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert_eq!(
        (a.active_calls, a.max_concurrency, a.queue_depth),
        (2, 2, 1)
    );
    assert_eq!(a.queued_by_class[&TrafficClass::Background], 1);
    assert_eq!(a.total_admissions, 2);
    assert_eq!(a.total_waited, 1);
    assert_eq!(
        serde_json::to_value(s.admission_snapshot()).unwrap(),
        before
    );
    cancel.store(true, Ordering::Release);
    assert!(matches!(waiting.await, Err(SchedulerError::Cancelled)));
    drop((one, two));
    let snap = s.operational_snapshot();
    let a = snap
        .admission
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert_eq!((a.active_calls, a.queue_depth), (0, 0));
}
#[test]
fn operational_usage_partial_quota_constraints_and_retry_hint_are_independent() {
    let (s, _, calls) = fixture(None);
    s.rate.set_policy("groq", daily()).unwrap();
    let observation = s.telemetry.attempt("groq");
    let guard = s
        .rate
        .reserve("groq", "model-a", 0, None, &AtomicBool::new(false))
        .unwrap();
    observation.attach_rate(guard.handle());
    assert!(observation.started());
    observation.observed_usage([(UsageDimension::InputTokens, Some(12))]);
    observation.retry_hint(Some(Timing::DelayMs(9_999)));
    observation.quota(
        QuotaScope::Model {
            model: "model-a".into(),
        },
        QuotaDimension::RequestsPerDay,
        Some(14_400),
        None,
        None,
    );
    observation.finished(None);
    s.resilience.authorize("groq", 0).unwrap().finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(50),
        }),
    );
    let snap = s.operational_snapshot();
    let t = snap
        .telemetry
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap();
    assert!(matches!(
        t.usage[&UsageDimension::InputTokens].observed,
        Fact::Known { value: 12, .. }
    ));
    assert_eq!(t.usage[&UsageDimension::InputTokens].reporting_requests, 1);
    assert!(matches!(
        t.usage[&UsageDimension::TotalTokens].observed,
        Fact::Unknown
    ));
    assert!(matches!(
        t.retry_hint,
        Fact::Known {
            value: Timing::DelayMs(9_999),
            ..
        }
    ));
    assert!(t.updated_age_ms.is_some());
    let q = &t.quotas[1].dimensions[&QuotaDimension::RequestsPerDay];
    assert!(matches!(
        q.limit,
        Fact::Known {
            value: 14_400,
            provenance: Provenance::ProviderHeader,
            ..
        }
    ));
    assert!(matches!(
        (&q.remaining, &q.reset),
        (Fact::Unknown, Fact::Unknown)
    ));
    let r = snap.rate.iter().find(|p| p.provider_id == "groq").unwrap();
    assert_eq!(r.pending_reservations, 1);
    let token = r
        .constraints
        .iter()
        .find(|c| c.dimension == QuotaDimension::TokensPerDay)
        .unwrap();
    assert_eq!(token.unaccounted_token_calls, 1);
    assert_eq!(token.effective_remaining, None);
    assert!(r
        .constraints
        .iter()
        .any(|c| c.source == super::rate::ConstraintSource::ExternalFact
            && c.scope
                == QuotaScope::Model {
                    model: "model-a".into()
                }));
    assert_eq!(
        snap.resilience
            .iter()
            .find(|p| p.provider_id == "groq")
            .unwrap()
            .cooldown_remaining_ms,
        50
    );
    drop(guard);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn operational_saturation_and_private_provider_data_cannot_enter_json() {
    let (s, clock, _) = fixture(None);
    for value in [MAX_FACT_VALUE, 1] {
        let obs = s.telemetry.attempt("groq");
        assert!(obs.started());
        obs.observed_usage([(UsageDimension::ThoughtTokens, Some(value))]);
    }
    s.rate.set_policy("groq", daily()).unwrap();
    let obs = s.telemetry.attempt("groq");
    let guard = s
        .rate
        .reserve(
            "groq",
            "model-a",
            0,
            Some(TokenUpperBound::explicit_total(100).unwrap()),
            &AtomicBool::new(false),
        )
        .unwrap();
    obs.attach_rate(guard.handle());
    assert!(obs.started());
    obs.observed_usage([(UsageDimension::TotalTokens, Some(MAX_FACT_VALUE))]);
    drop(guard);
    let obs2 = s.telemetry.attempt("groq");
    let guard2 = s
        .rate
        .reserve("groq", "model-a", 0, None, &AtomicBool::new(false))
        .unwrap();
    obs2.attach_rate(guard2.handle());
    assert!(obs2.started());
    obs2.observed_usage([(UsageDimension::TotalTokens, Some(1))]);
    drop(guard2);
    clock.0.store(0, Ordering::SeqCst);
    s.resilience.authorize("groq", 0).unwrap().finish(
        true,
        Some(&ProviderError::RateLimited {
            retry_after_ms: Some(MAX_FACT_VALUE + 1),
        }),
    );
    let snap = s.operational_snapshot();
    assert!(
        snap.telemetry
            .iter()
            .find(|p| p.provider_id == "groq")
            .unwrap()
            .usage[&UsageDimension::ThoughtTokens]
            .saturated
    );
    assert!(snap
        .rate
        .iter()
        .find(|p| p.provider_id == "groq")
        .unwrap()
        .constraints
        .iter()
        .any(|c| c.saturated));
    assert!(
        snap.resilience
            .iter()
            .find(|p| p.provider_id == "groq")
            .unwrap()
            .saturated
    );
    let json = serde_json::to_string(&snap).unwrap();
    for marker in [
        "sk-operational",
        "Bearer",
        "private_account_id",
        "private_prompt",
        "private_output",
        "private_reasoning",
        "raw_body",
        "Authorization",
        "unlock_material",
    ] {
        assert!(!json.contains(marker), "private marker {marker}");
    }
}
#[test]
fn operational_persistence_failed_survives_read_with_database_inaccessible() {
    let dir = std::env::temp_dir().join(format!(
        "lr8e-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::for_test(dir.join("state.sqlite3"));
    let (s, _, calls) = fixture(Some(db.clone()));
    s.rate.set_policy("groq", daily()).unwrap();
    let conn = db.open().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_rate BEFORE UPDATE ON cognitive_rate_state BEGIN SELECT RAISE(FAIL,'synthetic private marker'); END;").unwrap();
    assert!(matches!(
        s.rate
            .reserve("groq", "model-a", 0, None, &AtomicBool::new(false)),
        Err(SchedulerError::RateStateUnavailable)
    ));
    drop(conn);
    std::fs::remove_dir_all(&dir).unwrap();
    for _ in 0..10 {
        let snap = s.operational_snapshot();
        let r = snap.rate.iter().find(|p| p.provider_id == "groq").unwrap();
        assert!(r.persistence_failed);
        assert_eq!(r.policy, daily());
        assert!(!serde_json::to_string(&snap)
            .unwrap()
            .contains("synthetic private marker"));
    }
    assert!(!dir.exists());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn operational_existing_policy_update_preserves_unedited_sections_and_accounting() {
    let (s, _, _) = fixture(None);
    s.rate.set_policy("groq", daily()).unwrap();
    let guard = s
        .rate
        .reserve("groq", "model-a", 0, None, &AtomicBool::new(false))
        .unwrap();
    guard.handle().started(None).unwrap();
    drop(guard);
    let mut policy = daily();
    policy.daily_budget.as_mut().unwrap().max_requests = Some(20);
    s.rate.set_policy("groq", policy.clone()).unwrap();
    let snap = s.operational_snapshot();
    let r = snap.rate.iter().find(|p| p.provider_id == "groq").unwrap();
    assert_eq!(r.policy, policy);
    assert!(r
        .constraints
        .iter()
        .filter(|c| matches!(
            c.dimension,
            QuotaDimension::RequestsPerMinute | QuotaDimension::RequestsPerDay
        ))
        .all(|c| c.consumed == 1));
    assert_eq!(r.pending_reservations, 0);
}
#[test]
fn operational_command_permissions_and_memory_only_dependency_boundary() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/settings-ai.json")).unwrap();
    let permission = "allow-get-provider-operational-snapshot";
    assert!(capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == permission));
    for content in [
        include_str!("../../capabilities/main-window.json"),
        include_str!("../../capabilities/settings-general.json"),
    ] {
        assert!(!content.contains(permission));
    }
    let source = include_str!("settings.rs");
    let command = source
        .split("pub async fn get_provider_operational_snapshot(")
        .nth(1)
        .unwrap()
        .split("#[tauri::command]")
        .next()
        .unwrap();
    for forbidden in [
        "Database",
        "SecretStore",
        "AppHandle",
        ".open(",
        "catalog::",
        "execute",
        "credentials",
    ] {
        assert!(!command.contains(forbidden), "dependency {forbidden}");
    }
    assert!(command.contains("State<'_, Arc<ProviderRuntime>>"));
}
