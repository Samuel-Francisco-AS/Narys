use super::{
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    scheduler::Scheduler,
    telemetry::*,
    types::*,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn number(fact: &Fact<u64>) -> Option<u64> {
    match fact {
        Fact::Known { value, .. } => Some(*value),
        Fact::Unknown => None,
    }
}
fn counter(snapshot: &ProviderTelemetrySnapshot, dim: UsageDimension) -> Option<u64> {
    number(&snapshot.usage[&dim].observed)
}
fn snapshot(s: &Scheduler, id: &str) -> ProviderTelemetrySnapshot {
    s.telemetry_snapshot()
        .into_iter()
        .find(|s| s.provider_id == id)
        .unwrap()
}
fn measured() -> ProviderUsage {
    ProviderUsage {
        calls: 99,
        input_tokens: 7,
        output_tokens: 3,
        total_tokens: Some(10),
        thought_tokens: Some(1),
        output_tokens_measured: true,
    }
}
#[derive(Clone)]
enum Action {
    Success(ProviderUsage),
    Error(ProviderError),
    CancelAfterStart,
    PreflightFailure,
    RepeatUsage,
    PartialFailure,
}
struct Fake(Mutex<VecDeque<Action>>);
impl Provider for Fake {
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { panic!("observed boundary required") })
    }
    fn execute_observed<'a>(
        &'a self,
        _: &'a ProviderRequest,
        cancelled: &'a AtomicBool,
        on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        observation: &'a InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if cancelled.load(Ordering::Acquire) {
                return Err(ProviderError::Cancelled);
            }
            let action = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected invocation");
            if matches!(action, Action::PreflightFailure) {
                return Err(ProviderError::Authentication);
            }
            observation.started();
            match action {
                Action::Error(e) => Err(e),
                Action::CancelAfterStart => {
                    cancelled.store(true, Ordering::Release);
                    Err(ProviderError::Cancelled)
                }
                Action::PartialFailure => {
                    on_chunk(ProviderChunk {
                        text: "private-output-marker".into(),
                    })?;
                    Err(ProviderError::Timeout)
                }
                Action::Success(usage) => {
                    observation.usage(usage);
                    Ok(ProviderResponse {
                        text: "private-output-marker".into(),
                        usage,
                    })
                }
                Action::RepeatUsage => {
                    observation.started();
                    observation.usage(measured());
                    observation.usage(measured());
                    let usage = ProviderUsage {
                        input_tokens: 9,
                        total_tokens: Some(12),
                        ..measured()
                    };
                    observation.usage(usage);
                    Ok(ProviderResponse {
                        text: "private-output-marker".into(),
                        usage,
                    })
                }
                Action::PreflightFailure => unreachable!(),
            }
        })
    }
}
fn scheduler(actions: &[(&str, Vec<Action>)]) -> Scheduler {
    let mut registry = ProviderRegistry::default();
    for (index, (id, actions)) in actions.iter().enumerate() {
        registry
            .register(
                ProviderConfig {
                    id: (*id).into(),
                    enabled: true,
                    priority: index as u16,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(Fake(Mutex::new(actions.clone().into()))),
            )
            .unwrap();
    }
    Scheduler::new(registry)
}
fn request(ids: &[&str], selection: ProviderSelection) -> ProviderTaskRequest {
    ProviderTaskRequest {
        allocation_policy: Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
            crate::cognitive_resources::provider_allocation_default(),
            None,
        )),
        traffic_class: crate::cognition::admission::TrafficClass::ForegroundInteractive,
        mode: InvocationMode::default(),
        input: "private-prompt-marker".into(),
        internal_system_instruction: Some("private-reasoning-marker".into()),
        history: vec![],
        context: Arc::new(super::orchestrator::technical_context()),
        max_output_tokens: Some(20),
        selection,
        targets: ids
            .iter()
            .map(|id| ProviderTarget {
                provider_id: (*id).into(),
                invocation: ProviderInvocationConfig {
                    model: "private-model-marker".into(),
                    thinking_level: None,
                    timeouts: None,
                },
            })
            .collect(),
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    }
}
fn run(
    s: &Scheduler,
    ids: &[&str],
    selection: ProviderSelection,
) -> Result<TaskResult, SchedulerError> {
    tauri::async_runtime::block_on(s.run(
        request(ids, selection),
        TaskBudget {
            max_provider_calls: 4,
            max_output_tokens: Some(20),
        },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    ))
}
#[test]
fn telemetry_success_isolated_exactly_one_request_and_usage_once() {
    let s = scheduler(&[("a", vec![Action::Success(measured())]), ("b", vec![])]);
    let result = run(&s, &["a", "b"], ProviderSelection::Preferred).unwrap();
    assert_eq!(result.provider_id, "a");
    let a = snapshot(&s, "a");
    let b = snapshot(&s, "b");
    assert_eq!(counter(&a, UsageDimension::Requests), Some(1));
    assert_eq!(counter(&a, UsageDimension::InputTokens), Some(7));
    assert_eq!(counter(&a, UsageDimension::OutputTokens), Some(3));
    assert_eq!(counter(&a, UsageDimension::TotalTokens), Some(10));
    assert_eq!(counter(&b, UsageDimension::Requests), Some(0));
    assert_eq!(counter(&b, UsageDimension::InputTokens), None);
    assert_eq!(a.usage[&UsageDimension::OutputTokens].reporting_requests, 1);
}
#[test]
fn telemetry_real_retry_counts_two_requests_without_fictitious_failed_usage() {
    let s = scheduler(&[(
        "a",
        vec![
            Action::Error(ProviderError::Timeout),
            Action::Success(measured()),
        ],
    )]);
    assert_eq!(
        run(&s, &["a"], ProviderSelection::Fixed("a".into()))
            .unwrap()
            .usage
            .retries,
        1
    );
    let a = snapshot(&s, "a");
    assert_eq!(counter(&a, UsageDimension::Requests), Some(2));
    assert_eq!(counter(&a, UsageDimension::TotalTokens), Some(10));
    assert_eq!(a.usage[&UsageDimension::TotalTokens].reporting_requests, 1);
}
#[test]
fn telemetry_fallback_only_counts_invoked_providers_and_preserves_cooldown() {
    for error in [
        ProviderError::RateLimited {
            retry_after_ms: Some(60_000),
        },
        ProviderError::Unavailable {
            retry_after_ms: Some(60_000),
        },
    ] {
        let s = scheduler(&[
            ("a", vec![Action::Error(error)]),
            (
                "b",
                vec![Action::Success(measured()), Action::Success(measured())],
            ),
            ("c", vec![]),
        ]);
        let result = run(&s, &["a", "b", "c"], ProviderSelection::Preferred).unwrap();
        assert_eq!(result.provider_id, "b");
        assert_eq!(result.usage.fallbacks, 1);
        assert!(s.status().iter().find(|x| x.id == "a").unwrap().cooldown_ms > 0);
        assert_eq!(
            counter(&snapshot(&s, "a"), UsageDimension::Requests),
            Some(1)
        );
        assert_eq!(
            counter(&snapshot(&s, "a"), UsageDimension::OutputTokens),
            None
        );
        assert_eq!(
            counter(&snapshot(&s, "b"), UsageDimension::Requests),
            Some(1)
        );
        assert_eq!(
            counter(&snapshot(&s, "c"), UsageDimension::Requests),
            Some(0)
        );
        assert_eq!(
            run(&s, &["a", "b", "c"], ProviderSelection::Preferred)
                .unwrap()
                .provider_id,
            "b"
        );
        assert_eq!(
            counter(&snapshot(&s, "a"), UsageDimension::Requests),
            Some(1)
        );
        assert_eq!(
            run(&s, &["a"], ProviderSelection::Fixed("a".into())).unwrap_err(),
            SchedulerError::NoProvider
        );
    }
}
#[test]
fn telemetry_absent_usage_stays_unknown_and_d3_accounting_stays_conservative() {
    let s = scheduler(&[(
        "a",
        vec![
            Action::Error(ProviderError::Timeout),
            Action::Success(ProviderUsage::default()),
        ],
    )]);
    let result = tauri::async_runtime::block_on(s.run_with_retry_conservative_output(
        request(&["a"], ProviderSelection::Fixed("a".into())),
        TaskBudget {
            max_provider_calls: 2,
            max_output_tokens: Some(20),
        },
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 0,
        },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.usage.output_tokens, 0);
    assert!(!result.usage.output_tokens_measured);
    assert_eq!(result.usage.output_tokens_accounted, 20);
    let a = snapshot(&s, "a");
    assert_eq!(counter(&a, UsageDimension::Requests), Some(2));
    for dim in [
        UsageDimension::InputTokens,
        UsageDimension::OutputTokens,
        UsageDimension::TotalTokens,
    ] {
        assert_eq!(counter(&a, dim), None);
    }
}
#[test]
fn telemetry_cancel_before_invocation_and_sink_failure_never_count() {
    for sink_failure in [false, true] {
        let s = scheduler(&[("a", vec![])]);
        let cancelled = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(s.run(
            request(&["a"], ProviderSelection::Fixed("a".into())),
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(20),
            },
            &cancelled,
            &mut |_| {
                if sink_failure {
                    Err(SchedulerError::EventSinkClosed)
                } else {
                    cancelled.store(true, Ordering::Release);
                    Ok(())
                }
            },
        ));
        assert_eq!(
            result.unwrap_err(),
            if sink_failure {
                SchedulerError::EventSinkClosed
            } else {
                SchedulerError::Cancelled
            }
        );
        assert_eq!(
            counter(&snapshot(&s, "a"), UsageDimension::Requests),
            Some(0)
        );
    }
}
#[test]
fn telemetry_cancel_after_start_counts_request_without_tokens() {
    let s = scheduler(&[("a", vec![Action::CancelAfterStart]), ("b", vec![])]);
    assert_eq!(
        run(&s, &["a", "b"], ProviderSelection::Preferred).unwrap_err(),
        SchedulerError::Cancelled
    );
    assert_eq!(
        counter(&snapshot(&s, "a"), UsageDimension::Requests),
        Some(1)
    );
    assert_eq!(
        counter(&snapshot(&s, "a"), UsageDimension::OutputTokens),
        None
    );
    assert_eq!(
        counter(&snapshot(&s, "b"), UsageDimension::Requests),
        Some(0)
    );
}
#[test]
fn telemetry_preflight_failure_does_not_count_remote_request() {
    let s = scheduler(&[("a", vec![Action::PreflightFailure])]);
    assert_eq!(
        run(&s, &["a"], ProviderSelection::Fixed("a".into())).unwrap_err(),
        SchedulerError::Provider(ProviderError::Authentication)
    );
    assert_eq!(
        counter(&snapshot(&s, "a"), UsageDimension::Requests),
        Some(0)
    );
}
#[test]
fn telemetry_repeated_cumulative_usage_is_idempotent() {
    let s = scheduler(&[("a", vec![Action::RepeatUsage])]);
    run(&s, &["a"], ProviderSelection::Preferred).unwrap();
    let a = snapshot(&s, "a");
    assert_eq!(counter(&a, UsageDimension::Requests), Some(1));
    assert_eq!(counter(&a, UsageDimension::InputTokens), Some(9));
    assert_eq!(counter(&a, UsageDimension::OutputTokens), Some(3));
    assert_eq!(a.usage[&UsageDimension::InputTokens].reporting_requests, 1);
}
#[test]
fn telemetry_unknown_dimensions_and_invalid_metadata_are_not_zero() {
    let store = TelemetryStore::new(["a".into()]);
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::TokensPerMinute,
        Some(u64::MAX),
        None,
        None,
        Provenance::ProviderHeader,
    );
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerDay,
        Some(2),
        Some(3),
        None,
        Provenance::ProviderHeader,
    );
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::Concurrency,
        None,
        None,
        Some(Timing::UnixMs(u64::MAX)),
        Provenance::ProviderHeader,
    );
    let a = &store.snapshots()[0];
    for quota in a.quotas[0].dimensions.values() {
        assert_eq!(quota, &QuotaSnapshot::default());
    }
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::RequestsPerMinute,
        Some(0),
        Some(0),
        Some(Timing::DelayMs(0)),
        Provenance::UserConfiguration,
    );
    let a = &store.snapshots()[0];
    assert_eq!(
        number(&a.quotas[0].dimensions[&QuotaDimension::RequestsPerMinute].limit),
        Some(0)
    );
    assert!(matches!(
        a.quotas[0].dimensions[&QuotaDimension::RequestsPerMinute].limit,
        Fact::Known {
            provenance: Provenance::UserConfiguration,
            ..
        }
    ));
    assert_eq!(a.retry_hint, Fact::Unknown);
}
#[test]
fn telemetry_extreme_usage_saturates_and_invalid_values_are_ignored() {
    let store = TelemetryStore::new(["a".into()]);
    for _ in 0..2 {
        let attempt = store.attempt("a");
        attempt.started();
        attempt.observed_usage([
            (UsageDimension::InputTokens, Some(MAX_FACT_VALUE)),
            (UsageDimension::OutputTokens, Some(u64::MAX)),
        ]);
    }
    let a = &store.snapshots()[0];
    assert_eq!(
        counter(a, UsageDimension::InputTokens),
        Some(MAX_FACT_VALUE)
    );
    assert!(a.usage[&UsageDimension::InputTokens].saturated);
    assert_eq!(counter(a, UsageDimension::OutputTokens), None);
}
#[test]
fn telemetry_concurrent_snapshots_are_consistent_and_monotonic() {
    let store = Arc::new(TelemetryStore::new(["a".into(), "b".into()]));
    let barrier = Arc::new(std::sync::Barrier::new(5));
    let mut writers = vec![];
    for _ in 0..4 {
        let store = store.clone();
        let barrier = barrier.clone();
        writers.push(std::thread::spawn(move || {
            barrier.wait();
            for _ in 0..100 {
                let a = store.attempt("a");
                a.started();
                a.usage(measured());
            }
        }));
    }
    barrier.wait();
    let mut previous = 0;
    for _ in 0..400 {
        let all = store.snapshots();
        let a = &all[0];
        let count = counter(a, UsageDimension::Requests).unwrap();
        assert!(count >= previous);
        previous = count;
        if let Some(total) = counter(a, UsageDimension::TotalTokens) {
            assert_eq!(
                counter(a, UsageDimension::InputTokens).unwrap()
                    + counter(a, UsageDimension::OutputTokens).unwrap(),
                total
            );
            assert_eq!(
                total,
                a.usage[&UsageDimension::TotalTokens].reporting_requests * 10
            );
        }
        assert_eq!(counter(&all[1], UsageDimension::Requests), Some(0));
    }
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(
        counter(&store.snapshots()[0], UsageDimension::Requests),
        Some(400)
    );
}
#[test]
fn telemetry_quotas_do_not_change_ranking_but_lr8c_enforces_selected_capacity() {
    for mode in [
        ProviderSelection::Auto,
        ProviderSelection::Preferred,
        ProviderSelection::Fixed("a".into()),
    ] {
        let s = scheduler(&[
            ("a", vec![Action::Success(measured())]),
            ("b", vec![Action::Success(measured())]),
        ]);
        let targets = request(&["a", "b"], mode.clone()).targets;
        let before = s
            .ranked_provider_ids(
                &mode,
                &targets,
                &ProviderCapabilities::text_stream(),
                &InvocationMode::default(),
                Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                    crate::cognitive_resources::provider_allocation_default(),
                    None,
                ))
                .as_ref(),
            )
            .unwrap();
        for dim in [
            QuotaDimension::RequestsPerMinute,
            QuotaDimension::TokensPerMinute,
            QuotaDimension::RequestsPerDay,
            QuotaDimension::TokensPerDay,
            QuotaDimension::Concurrency,
        ] {
            s.telemetry.observe_quota(
                "a",
                QuotaScope::Provider,
                dim,
                Some(0),
                Some(0),
                None,
                Provenance::ProviderHeader,
            );
        }
        let a = s.telemetry.attempt("a");
        a.started();
        a.finished(Some(&ProviderError::Fatal));
        if mode == ProviderSelection::Auto {
            assert_eq!(
                s.ranked_provider_ids(
                    &mode,
                    &targets,
                    &ProviderCapabilities::text_stream(),
                    &InvocationMode::default(),
                    Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                        crate::cognitive_resources::provider_allocation_default(),
                        None
                    ))
                    .as_ref(),
                )
                .unwrap(),
                vec!["b"]
            );
            assert_eq!(run(&s, &["a", "b"], mode).unwrap().provider_id, "b");
            continue;
        }
        assert_eq!(
            s.ranked_provider_ids(
                &mode,
                &targets,
                &ProviderCapabilities::text_stream(),
                &InvocationMode::default(),
                Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                    crate::cognitive_resources::provider_allocation_default(),
                    None
                ))
                .as_ref(),
            )
            .unwrap(),
            before
        );
        assert_eq!(
            run(&s, &["a", "b"], mode).unwrap_err(),
            SchedulerError::RateCapacityExceeded
        );
    }
}
#[test]
fn telemetry_partial_output_preserves_no_retry_no_fallback() {
    let s = scheduler(&[("a", vec![Action::PartialFailure]), ("b", vec![])]);
    assert_eq!(
        run(&s, &["a", "b"], ProviderSelection::Preferred).unwrap_err(),
        SchedulerError::Provider(ProviderError::Timeout)
    );
    assert_eq!(
        counter(&snapshot(&s, "a"), UsageDimension::Requests),
        Some(1)
    );
    assert_eq!(
        counter(&snapshot(&s, "b"), UsageDimension::Requests),
        Some(0)
    );
}
#[test]
fn telemetry_snapshot_has_no_sensitive_input_output_model_or_headers() {
    let s = scheduler(&[("a", vec![Action::Success(measured())])]);
    run(&s, &["a"], ProviderSelection::Auto).unwrap();
    let json = serde_json::to_string(&s.telemetry_snapshot()).unwrap();
    for marker in [
        "private-prompt-marker",
        "private-output-marker",
        "private-reasoning-marker",
        "private-model-marker",
        "Bearer",
        "Cookie",
        "Authorization",
        "apiKey",
    ] {
        assert!(!json.contains(marker));
    }
}
#[test]
fn telemetry_retry_after_positive_negative_and_extreme_fixtures() {
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    for (value, expected) in [
        ("2", Some(2000)),
        ("0", Some(0)),
        ("18446744073709551615", Some(604800000)),
        ("-1", None),
        ("1.5", None),
        ("garbage", None),
        ("18446744073709551616", None),
        ("Thu, 01 Jan 1970 00:00:00 GMT", None),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        let store = TelemetryStore::new(["a".into()]);
        let a = store.attempt("a");
        a.started();
        let parsed = super::transport::retry_after_ms(&headers);
        assert_eq!(parsed, expected);
        a.retry_hint(super::transport::factual_retry_after(&headers));
        assert_eq!(
            matches!(store.snapshots()[0].retry_hint, Fact::Known { .. }),
            (expected.is_some() && value != "18446744073709551615")
                || value == "Thu, 01 Jan 1970 00:00:00 GMT"
        );
        assert!(store.snapshots()[0].quotas[0]
            .dimensions
            .values()
            .all(|q| q == &QuotaSnapshot::default()));
    }
}

use crate::security::secrets::{SecretError, SecretKey, SecretStore, UnlockKeyStore};
#[derive(Default)]
struct Keys(Mutex<Option<Vec<u8>>>);
impl UnlockKeyStore for Keys {
    fn load(&self) -> Result<Option<Vec<u8>>, SecretError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn store(&self, value: &[u8]) -> Result<(), SecretError> {
        *self.0.lock().unwrap() = Some(value.to_vec());
        Ok(())
    }
    fn delete(&self) -> Result<(), SecretError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
struct HttpFixture {
    store: Arc<SecretStore>,
    dir: std::path::PathBuf,
}
impl HttpFixture {
    fn new(credentials: bool) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lr8a-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Arc::new(SecretStore::with_key_store(
            dir.clone(),
            Arc::new(Keys::default()),
        ));
        if credentials {
            store
                .set_secrets(&[
                    (SecretKey::GeminiApiKey, b"synthetic-secret-marker".to_vec()),
                    (SecretKey::GroqApiKey, b"synthetic-secret-marker".to_vec()),
                    (
                        SecretKey::MistralApiKey,
                        b"synthetic-secret-marker".to_vec(),
                    ),
                    (
                        SecretKey::CloudflareApiToken,
                        b"synthetic-secret-marker".to_vec(),
                    ),
                    (
                        SecretKey::CloudflareAccountId,
                        b"synthetic-account-marker".to_vec(),
                    ),
                ])
                .unwrap();
        }
        Self { store, dir }
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn remote(id: &str, endpoint: String, store: Arc<SecretStore>) -> Arc<dyn Provider> {
    match id {
        "gemini" => Arc::new(
            super::gemini::GeminiProvider::new(
                super::gemini::GeminiConfig {
                    endpoint,
                    ..Default::default()
                },
                store,
            )
            .unwrap(),
        ),
        "groq" => Arc::new(
            super::groq::GroqProvider::new(
                super::groq::GroqConfig {
                    endpoint,
                    ..Default::default()
                },
                store,
            )
            .unwrap(),
        ),
        "mistral" => Arc::new(
            super::mistral::MistralProvider::new(
                super::mistral::MistralConfig {
                    endpoint,
                    ..Default::default()
                },
                store,
            )
            .unwrap(),
        ),
        "cloudflare" => Arc::new(
            super::cloudflare::CloudflareProvider::new(
                super::cloudflare::CloudflareConfig {
                    endpoint,
                    ..Default::default()
                },
                store,
            )
            .unwrap(),
        ),
        _ => unreachable!(),
    }
}
fn remote_scheduler(id: &str, endpoint: String, store: Arc<SecretStore>) -> Scheduler {
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: id.into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::with_structured_output(),
            },
            remote(id, endpoint, store),
        )
        .unwrap();
    Scheduler::new(registry)
}
fn remote_request(id: &str) -> ProviderTaskRequest {
    let mut r = request(&[id], ProviderSelection::Fixed(id.into()));
    r.targets[0].invocation.model = match id {
        "gemini" => super::gemini::MODEL,
        "groq" => super::groq::MODEL,
        "mistral" => super::mistral::MODEL,
        "cloudflare" => super::cloudflare::MODEL,
        _ => unreachable!(),
    }
    .into();
    r.targets[0].invocation.timeouts = Some(ProviderTimeouts {
        request_timeout_ms: 10_000,
        stream_idle_timeout_ms: 5000,
    });
    r
}
fn run_remote(s: &Scheduler, r: ProviderTaskRequest) -> Result<TaskResult, SchedulerError> {
    tauri::async_runtime::block_on(s.run_with_retry(
        r,
        TaskBudget {
            max_provider_calls: 1,
            max_output_tokens: Some(20),
        },
        RetryPolicy {
            enabled: false,
            max_retries: 0,
            initial_backoff_ms: 0,
        },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    ))
}
fn sse(id: &str, raw_usage: &str) -> String {
    if id == "gemini" {
        format!("event: step.start\ndata: {{\"index\":0,\"step\":{{\"type\":\"model_output\"}}}}\n\nevent: step.delta\ndata: {{\"index\":0,\"delta\":{{\"type\":\"text\",\"text\":\"private-output-marker\"}}}}\n\nevent: interaction.completed\ndata: {{\"interaction\":{{\"status\":\"completed\",\"usage\":{raw_usage}}}}}\n\ndata: [DONE]\n\n")
    } else {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"private-output-marker\"}},\"finish_reason\":\"stop\"}}]}}\n\ndata: {{\"choices\":[],\"usage\":{raw_usage}}}\n\ndata: [DONE]\n\n")
    }
}
#[test]
fn telemetry_production_all_adapters_http_usage_and_preflight_boundaries() {
    let credentials = HttpFixture::new(true);
    let empty = HttpFixture::new(false);
    for id in ["gemini", "groq", "mistral", "cloudflare"] {
        let raw = if id == "gemini" {
            r#"{"total_input_tokens":7,"total_output_tokens":3,"total_tokens":10}"#
        } else {
            r#"{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}"#
        };
        let (url, server) = super::transport::test_support::server(
            "200 OK",
            &sse(id, raw),
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler(id, url, credentials.store.clone());
        run_remote(&s, remote_request(id)).unwrap();
        server.join().unwrap();
        let facts = snapshot(&s, id);
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1), "{id}");
        assert_eq!(
            counter(&facts, UsageDimension::OutputTokens),
            Some(3),
            "{id}"
        );
        assert!(facts.quotas[0]
            .dimensions
            .values()
            .all(|q| q == &QuotaSnapshot::default()));
        let json = serde_json::to_string(&facts).unwrap();
        for marker in [
            "synthetic-secret-marker",
            "synthetic-account-marker",
            "private-output-marker",
            "private-prompt-marker",
        ] {
            assert!(!json.contains(marker));
        }
        let s = remote_scheduler(id, "http://127.0.0.1:1".into(), empty.store.clone());
        assert_eq!(
            run_remote(&s, remote_request(id)).unwrap_err(),
            SchedulerError::Provider(ProviderError::Authentication)
        );
        assert_eq!(
            counter(&snapshot(&s, id), UsageDimension::Requests),
            Some(0)
        );
        let s = remote_scheduler(id, "not-a-url".into(), credentials.store.clone());
        assert_eq!(
            run_remote(&s, remote_request(id)).unwrap_err(),
            SchedulerError::Provider(ProviderError::Unavailable {
                retry_after_ms: None
            })
        );
        assert_eq!(
            counter(&snapshot(&s, id), UsageDimension::Requests),
            Some(0)
        );
    }
}
#[test]
fn telemetry_production_invalid_and_absent_usage_never_create_tokens() {
    let credentials = HttpFixture::new(true);
    for id in ["gemini", "groq", "mistral", "cloudflare"] {
        for raw in [
            "null",
            r#"{"prompt_tokens":7,"completion_tokens":3,"total_tokens":1,"total_input_tokens":7,"total_output_tokens":3}"#,
            r#"{"prompt_tokens":-1,"completion_tokens":3,"total_tokens":10,"total_input_tokens":-1,"total_output_tokens":3}"#,
            r#"{"prompt_tokens":18446744073709551615,"completion_tokens":3,"total_tokens":10,"total_input_tokens":18446744073709551615,"total_output_tokens":3}"#,
        ] {
            let (url, server) = super::transport::test_support::server(
                "200 OK",
                &sse(id, raw),
                false,
                std::time::Duration::ZERO,
            );
            let s = remote_scheduler(id, url, credentials.store.clone());
            let _ = run_remote(&s, remote_request(id));
            server.join().unwrap();
            let facts = snapshot(&s, id);
            assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
            for dim in [
                UsageDimension::InputTokens,
                UsageDimension::OutputTokens,
                UsageDimension::TotalTokens,
            ] {
                assert_eq!(counter(&facts, dim), None, "{id}: {raw}");
            }
        }
    }
}
#[test]
fn telemetry_production_structured_groq_counts_once_even_if_core_rejects_budget() {
    let f = HttpFixture::new(true);
    for output in [3, 21] {
        let body = serde_json::json!({"choices":[{"message":{"content":"{}"},"finish_reason":"stop"}], "usage":{"prompt_tokens":7,"completion_tokens":output,"total_tokens":7+output}}).to_string();
        let (url, server) = super::transport::test_support::server(
            "200 OK",
            &body,
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("groq", url, f.store.clone());
        let mut r = remote_request("groq");
        r.mode = InvocationMode {
            output: OutputContract::JsonSchema {
                name: "PlanV1".into(),
                schema: crate::agents::planner::output_schema(),
                max_bytes: 1000,
            },
            transport: TransportMode::NonStreaming,
        };
        r.required_capabilities = ProviderCapabilities::structured();
        let result = run_remote(&s, r);
        server.join().unwrap();
        if output == 21 {
            assert_eq!(result.unwrap_err(), SchedulerError::BudgetExceeded);
        } else {
            result.unwrap();
        }
        let facts = snapshot(&s, "groq");
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        assert_eq!(
            counter(&facts, UsageDimension::OutputTokens),
            Some(output as u64)
        );
    }
}
#[test]
fn telemetry_production_retry_after_is_normalized_without_changing_cooldown() {
    let f = HttpFixture::new(true);
    for id in ["gemini", "groq", "mistral", "cloudflare"] {
        for (header, expected) in [("2", Some(2000)), ("-1", None)] {
            let status = format!("429 Too Many Requests\r\nRetry-After: {header}\r\nX-Untrusted-Quota: 999\r\nSet-Cookie: private-cookie-marker");
            let (url, server) = super::transport::test_support::server(
                &status,
                "{}",
                false,
                std::time::Duration::ZERO,
            );
            let s = remote_scheduler(id, url, f.store.clone());
            assert_eq!(
                run_remote(&s, remote_request(id)).unwrap_err(),
                SchedulerError::Provider(ProviderError::RateLimited {
                    retry_after_ms: expected
                })
            );
            server.join().unwrap();
            let facts = snapshot(&s, id);
            assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
            assert_eq!(
                facts.retry_hint,
                match expected {
                    Some(ms) => {
                        let Fact::Known {
                            observed_at_unix_ms,
                            ..
                        } = facts.retry_hint
                        else {
                            panic!("missing hint")
                        };
                        Fact::Known {
                            value: Timing::DelayMs(ms),
                            provenance: Provenance::ProviderHeader,
                            observed_at_unix_ms,
                        }
                    }
                    None => Fact::Unknown,
                }
            );
            assert!(s.status()[0].cooldown_ms > 0);
            assert!(facts.quotas[0]
                .dimensions
                .values()
                .all(|q| q == &QuotaSnapshot::default()));
            assert!(!serde_json::to_string(&facts)
                .unwrap()
                .contains("private-cookie-marker"));
        }
    }
}
#[test]
fn telemetry_production_cancellation_after_http_starts_counts_without_tokens() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::Duration,
    };
    let f = HttpFixture::new(true);
    for id in ["gemini", "groq", "mistral", "cloudflare"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 1024];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n").unwrap();
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        let s = Arc::new(remote_scheduler(id, url, f.store.clone()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = remote_request(id);
        let call_s = s.clone();
        let call_cancelled = cancelled.clone();
        let call = std::thread::spawn(move || {
            tauri::async_runtime::block_on(call_s.run_with_retry(
                request,
                TaskBudget {
                    max_provider_calls: 1,
                    max_output_tokens: Some(20),
                },
                RetryPolicy {
                    enabled: false,
                    max_retries: 0,
                    initial_backoff_ms: 0,
                },
                &call_cancelled,
                &mut |_| Ok(()),
            ))
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        cancelled.store(true, Ordering::Release);
        let result = call.join().unwrap();
        release_tx.send(()).unwrap();
        server.join().unwrap();
        assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
        let facts = snapshot(&s, id);
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        assert_eq!(counter(&facts, UsageDimension::OutputTokens), None);
    }
}

#[test]
fn telemetry_factual_retry_date_and_ambiguous_values_use_controlled_clock() {
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    use std::time::{Duration, UNIX_EPOCH};
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let mut headers = HeaderMap::new();
    let date = httpdate::fmt_http_date(now + Duration::from_secs(2));
    headers.insert(RETRY_AFTER, HeaderValue::from_str(&date).unwrap());
    assert_eq!(
        super::transport::factual_retry_after(&headers),
        Some(Timing::UnixMs(1_800_000_002_000))
    );
    let date = httpdate::fmt_http_date(now + Duration::from_secs(700_000));
    headers.insert(RETRY_AFTER, HeaderValue::from_str(&date).unwrap());
    assert_eq!(
        super::transport::factual_retry_after(&headers),
        Some(Timing::UnixMs(1_800_700_000_000))
    );
    for value in ["+1", "18446744073709551615", "1, 2"] {
        headers.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        assert_eq!(super::transport::factual_retry_after(&headers), None);
    }
    headers.insert(RETRY_AFTER, HeaderValue::from_static("2"));
    headers.append(RETRY_AFTER, HeaderValue::from_static("3"));
    assert_eq!(super::transport::factual_retry_after(&headers), None);
}
#[test]
fn telemetry_independent_usage_dimensions_preserve_unknown_and_zero() {
    let store = TelemetryStore::new(["a".into()]);
    let attempt = store.attempt("a");
    attempt.observed_usage([(UsageDimension::InputTokens, Some(1))]);
    assert_eq!(
        counter(&store.snapshots()[0], UsageDimension::InputTokens),
        None
    );
    attempt.started();
    attempt.observed_usage([
        (UsageDimension::InputTokens, Some(0)),
        (UsageDimension::OutputTokens, None),
    ]);
    let a = &store.snapshots()[0];
    assert_eq!(counter(a, UsageDimension::InputTokens), Some(0));
    assert_eq!(counter(a, UsageDimension::OutputTokens), None);
    assert_eq!(counter(a, UsageDimension::TotalTokens), None);
    assert_eq!(a.usage[&UsageDimension::InputTokens].reporting_requests, 1);
    let json = serde_json::to_value(a).unwrap();
    assert!(json["usage"]["input_tokens"]["observed"]["observedAtUnixMs"].is_number());
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::TokensPerMinute,
        Some(9),
        Some(8),
        None,
        Provenance::ProviderHeader,
    );
    store.observe_quota(
        "a",
        QuotaScope::Provider,
        QuotaDimension::TokensPerMinute,
        Some(u64::MAX),
        None,
        None,
        Provenance::ProviderHeader,
    );
    assert_eq!(
        number(&store.snapshots()[0].quotas[0].dimensions[&QuotaDimension::TokensPerMinute].limit),
        Some(9)
    );
}

fn scoped_dimensions<'a>(
    facts: &'a ProviderTelemetrySnapshot,
    scope: &QuotaScope,
) -> &'a std::collections::BTreeMap<QuotaDimension, QuotaSnapshot> {
    &facts
        .quotas
        .iter()
        .find(|q| &q.scope == scope)
        .unwrap()
        .dimensions
}

#[test]
fn lr8a_fix_quota_scopes_isolate_two_models_and_provider() {
    let store = TelemetryStore::new(["p".into()]);
    let a = QuotaScope::Model {
        model: "model/a".into(),
    };
    let b = QuotaScope::Model {
        model: "model/b".into(),
    };
    for (scope, limit) in [(a.clone(), 31), (b.clone(), 73), (QuotaScope::Provider, 11)] {
        store.observe_quota(
            "p",
            scope,
            QuotaDimension::RequestsPerDay,
            Some(limit),
            Some(limit - 1),
            None,
            Provenance::ProviderHeader,
        );
    }
    store.observe_quota(
        "p",
        a.clone(),
        QuotaDimension::RequestsPerDay,
        Some(41),
        None,
        None,
        Provenance::ProviderHeader,
    );
    let facts = &store.snapshots()[0];
    for (scope, limit) in [(a, 41), (b, 73), (QuotaScope::Provider, 11)] {
        let dimensions = scoped_dimensions(facts, &scope);
        assert_eq!(
            number(&dimensions[&QuotaDimension::RequestsPerDay].limit),
            Some(limit)
        );
        assert_eq!(
            dimensions[&QuotaDimension::TokensPerMinute],
            QuotaSnapshot::default()
        );
    }
    let json = serde_json::to_value(facts).unwrap();
    assert!(json["quotas"].is_array());
    assert_eq!(
        json["quotas"][1]["scope"],
        serde_json::json!({"kind":"model","model":"model/a"})
    );
}

#[test]
fn lr8a_fix_registry_ids_never_disappear_from_telemetry() {
    let id = "custom.provider/target:v1";
    let s = scheduler(&[(id, vec![Action::Success(measured())])]);
    assert_eq!(snapshot(&s, id).provider_id, id);
    run(&s, &[id], ProviderSelection::Fixed(id.into())).unwrap();
    assert_eq!(
        counter(&snapshot(&s, id), UsageDimension::Requests),
        Some(1)
    );
}

struct DefaultPreflight;
impl Provider for DefaultPreflight {
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async { Err(ProviderError::Authentication) })
    }
}
#[test]
fn lr8a_fix_default_provider_preflight_never_fabricates_request() {
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: "local".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(DefaultPreflight),
        )
        .unwrap();
    let s = Scheduler::new(registry);
    assert_eq!(
        run(&s, &["local"], ProviderSelection::Fixed("local".into())).unwrap_err(),
        SchedulerError::Provider(ProviderError::Authentication)
    );
    let facts = snapshot(&s, "local");
    assert_eq!(counter(&facts, UsageDimension::Requests), Some(0));
    assert_eq!(counter(&facts, UsageDimension::InputTokens), None);
    assert!(matches!(facts.last_outcome, Fact::Unknown));
}

#[test]
fn lr8a_fix_factual_retry_after_is_independent_of_operational_clamp() {
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    let mut headers = HeaderMap::new();
    for (value, expected) in [
        ("700000", Some(Timing::DelayMs(700_000_000))),
        (
            "9007199254740",
            Some(Timing::DelayMs(9_007_199_254_740_000)),
        ),
        ("18446744073709551615", None),
        ("9007199254741", None),
    ] {
        headers.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        assert_eq!(super::transport::factual_retry_after(&headers), expected);
        assert_eq!(
            super::transport::retry_after_ms(&headers),
            Some(604_800_000)
        );
    }
    let padded = format!(" \t{}2\t ", "0".repeat(160));
    headers.insert(RETRY_AFTER, HeaderValue::from_str(&padded).unwrap());
    assert_eq!(
        super::transport::factual_retry_after(&headers),
        Some(Timing::DelayMs(2000))
    );
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("Fri, 01 Jan 2100 00:00:00 GMT"),
    );
    assert_eq!(
        super::transport::factual_retry_after(&headers),
        Some(Timing::UnixMs(4_102_444_800_000))
    );
    assert_eq!(
        super::transport::retry_after_ms(&headers),
        Some(604_800_000)
    );
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT"),
    );
    assert_eq!(
        super::transport::factual_retry_after(&headers),
        Some(Timing::UnixMs(0))
    );
    assert_eq!(super::transport::retry_after_ms(&headers), None);
}

fn groq_fixture_request(reader: &mut impl std::io::Read) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let read = reader.read(&mut buffer).unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = std::str::from_utf8(&request[..end]).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if request.len() >= (end + 4).checked_add(length).unwrap() {
                break;
            }
        }
    }
    request
}

#[test]
fn lr8c_groq_http_fixture_consumes_body_split_after_headers_before_closing() {
    use std::{collections::VecDeque, io::Read};
    struct Segmented(VecDeque<Vec<u8>>);
    impl Read for Segmented {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let Some(bytes) = self.0.pop_front() else {
                return Ok(0);
            };
            assert!(bytes.len() <= buffer.len());
            buffer[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }
    }
    let header = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\n".to_vec();
    let mut reader = Segmented(VecDeque::from([
        header.clone(),
        b"ab".to_vec(),
        b"cde".to_vec(),
    ]));
    let request = groq_fixture_request(&mut reader);
    assert_eq!(request, [header, b"abcde".to_vec()].concat());
    assert!(
        reader.0.is_empty(),
        "fixture must not close with an unread body"
    );
}

// Two real HTTP responses on one endpoint exercise one Scheduler/store and two targets.
fn groq_two_models_server() -> (String, std::thread::JoinHandle<()>) {
    use std::{io::Write, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for (limit, remaining) in [(31, 29), (73, 70)] {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let _request = groq_fixture_request(&mut connection);
            let body = sse(
                "groq",
                r#"{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}"#,
            );
            write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nx-ratelimit-limit-requests: {limit}\r\nx-ratelimit-remaining-requests: {remaining}\r\nx-ratelimit-limit-tokens: {}\r\nx-ratelimit-remaining-tokens: {}\r\nx-ratelimit-reset-requests: 2m59.56s\r\nx-ratelimit-reset-tokens: 7.66s\r\nSet-Cookie: private-cookie-marker\r\nConnection: close\r\n\r\n{body}", body.len(), limit * 10, remaining * 10).unwrap();
        }
    });
    (url, server)
}
#[test]
fn lr8a_fix_groq_real_headers_isolate_models_and_map_rpd_tpm() {
    let f = HttpFixture::new(true);
    let (url, server) = groq_two_models_server();
    let s = remote_scheduler("groq", url, f.store.clone());
    for model in [super::groq::MODEL, "test/model-b"] {
        let mut r = remote_request("groq");
        r.targets[0].invocation.model = model.into();
        let result = run_remote(&s, r).unwrap();
        // Existing D3 measured/accounted ledger remains independent and unchanged.
        assert!(result.usage.output_tokens_measured);
        assert_eq!(result.usage.output_tokens, 3);
        assert_eq!(result.usage.output_tokens_accounted, 3);
        assert_eq!(result.usage.provider_calls, 1);
    }
    server.join().unwrap();
    let facts = snapshot(&s, "groq");
    for (model, limit, remaining) in [(super::groq::MODEL, 31, 29), ("test/model-b", 73, 70)] {
        let dims = scoped_dimensions(
            &facts,
            &QuotaScope::Model {
                model: model.into(),
            },
        );
        for (dim, multiplier) in [
            (QuotaDimension::RequestsPerDay, 1),
            (QuotaDimension::TokensPerMinute, 10),
        ] {
            assert_eq!(number(&dims[&dim].limit), Some(limit * multiplier));
            assert_eq!(number(&dims[&dim].remaining), Some(remaining * multiplier));
            assert_eq!(dims[&dim].reset, Fact::Unknown);
            assert!(matches!(
                dims[&dim].limit,
                Fact::Known {
                    provenance: Provenance::ProviderHeader,
                    ..
                }
            ));
        }
        assert_eq!(
            dims[&QuotaDimension::RequestsPerMinute],
            QuotaSnapshot::default()
        );
    }
    assert!(scoped_dimensions(&facts, &QuotaScope::Provider)
        .values()
        .all(|q| q == &QuotaSnapshot::default()));
    assert_eq!(counter(&facts, UsageDimension::Requests), Some(2));
    assert_eq!(counter(&facts, UsageDimension::TotalTokens), Some(20));
    assert_eq!(
        facts.usage[&UsageDimension::TotalTokens].reporting_requests,
        2
    );
    let json = serde_json::to_string(&facts).unwrap();
    for marker in [
        "private-cookie-marker",
        "synthetic-secret-marker",
        "synthetic-account-marker",
        "private-output-marker",
        "private-prompt-marker",
        "x-ratelimit",
    ] {
        assert!(!json.contains(marker));
    }
}

#[test]
fn lr8a_fix_groq_http_absent_partial_and_invalid_headers_stay_unknown() {
    let f = HttpFixture::new(true);
    let mut fixtures = vec![(String::new(), None, None),
        ("\r\nx-ratelimit-limit-requests: 37\r\nx-ratelimit-remaining-tokens: 19".into(), Some(37), Some(19)),
        ("\r\nx-ratelimit-limit-requests: 5\r\nx-ratelimit-remaining-requests: 6\r\nx-ratelimit-limit-tokens: 7\r\nx-ratelimit-remaining-tokens: 8".into(), None, None),
        ("\r\nx-ratelimit-limit-requests: 5\r\nx-ratelimit-limit-requests: 6\r\nx-ratelimit-remaining-tokens: 1, 2".into(), None, None)];
    for invalid in [
        "-1",
        "+1",
        "1.5",
        "18446744073709551616",
        "9007199254740992",
        "garbage",
    ] {
        fixtures.push((format!("\r\nx-ratelimit-limit-requests: {invalid}\r\nx-ratelimit-remaining-requests: {invalid}\r\nx-ratelimit-limit-tokens: {invalid}\r\nx-ratelimit-remaining-tokens: {invalid}"), None, None));
    }
    for (headers, limit, remaining) in fixtures {
        let (url, server) = super::transport::test_support::server(
            &format!("200 OK{headers}"),
            &sse("groq", "null"),
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("groq", url, f.store.clone());
        run_remote(&s, remote_request("groq")).unwrap();
        server.join().unwrap();
        let facts = snapshot(&s, "groq");
        // If both updates are invalid, no model scope is created: still unknown.
        let scope = QuotaScope::Model {
            model: super::groq::MODEL.into(),
        };
        let unknown = [
            QuotaDimension::RequestsPerDay,
            QuotaDimension::TokensPerMinute,
        ]
        .into_iter()
        .map(|d| (d, QuotaSnapshot::default()))
        .collect();
        let dims = facts
            .quotas
            .iter()
            .find(|q| q.scope == scope)
            .map(|q| &q.dimensions)
            .unwrap_or(&unknown);
        assert_eq!(
            number(&dims[&QuotaDimension::RequestsPerDay].limit),
            limit,
            "{headers}"
        );
        assert_eq!(
            number(&dims[&QuotaDimension::RequestsPerDay].remaining),
            None
        );
        assert_eq!(
            number(&dims[&QuotaDimension::TokensPerMinute].remaining),
            remaining,
            "{headers}"
        );
        assert_eq!(number(&dims[&QuotaDimension::TokensPerMinute].limit), None);
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        assert_eq!(counter(&facts, UsageDimension::OutputTokens), None);
    }
}

fn structured_groq_request() -> ProviderTaskRequest {
    let mut r = remote_request("groq");
    r.mode = InvocationMode {
        output: OutputContract::JsonSchema {
            name: "PlanV1".into(),
            schema: crate::agents::planner::output_schema(),
            max_bytes: 1000,
        },
        transport: TransportMode::NonStreaming,
    };
    r.required_capabilities = ProviderCapabilities::structured();
    r
}
#[test]
fn lr8a_fix_groq_rejected_non_streaming_keeps_valid_usage() {
    let f = HttpFixture::new(true);
    for (finish, content, expected) in [
        ("length", serde_json::json!("{}"), ProviderError::Incomplete),
        (
            "tool_calls",
            serde_json::json!("{}"),
            ProviderError::RequiresAction,
        ),
        ("stop", serde_json::Value::Null, ProviderError::Protocol),
    ] {
        let body =
            serde_json::json!({"choices":[{"finish_reason":finish,"message":{"content":content}}],
            "usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}})
            .to_string();
        let (url, server) = super::transport::test_support::server(
            "200 OK",
            &body,
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("groq", url, f.store.clone());
        assert_eq!(
            run_remote(&s, structured_groq_request()).unwrap_err(),
            SchedulerError::Provider(expected)
        );
        server.join().unwrap();
        let facts = snapshot(&s, "groq");
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        assert_eq!(counter(&facts, UsageDimension::InputTokens), Some(7));
        assert_eq!(counter(&facts, UsageDimension::OutputTokens), Some(3));
        assert_eq!(counter(&facts, UsageDimension::TotalTokens), Some(10));
        assert_eq!(
            facts.usage[&UsageDimension::TotalTokens].reporting_requests,
            1
        );
    }
}
#[test]
fn lr8a_fix_groq_invalid_usage_and_http_error_body_never_supply_tokens() {
    let f = HttpFixture::new(true);
    for raw in [
        "null",
        r#"{"prompt_tokens":-1,"completion_tokens":3,"total_tokens":10}"#,
        r#"{"prompt_tokens":7,"completion_tokens":1.5,"total_tokens":10}"#,
        r#"{"prompt_tokens":4294967296,"completion_tokens":3,"total_tokens":10}"#,
        r#"{"prompt_tokens":7,"completion_tokens":3}"#,
        r#"{"prompt_tokens":7,"completion_tokens":3,"total_tokens":9}"#,
    ] {
        let body = format!(
            r#"{{"choices":[{{"finish_reason":"length","message":{{"content":"{{}}"}}}}],"usage":{raw}}}"#
        );
        let (url, server) = super::transport::test_support::server(
            "200 OK",
            &body,
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("groq", url, f.store.clone());
        assert_eq!(
            run_remote(&s, structured_groq_request()).unwrap_err(),
            SchedulerError::Provider(ProviderError::Incomplete)
        );
        server.join().unwrap();
        let facts = snapshot(&s, "groq");
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        for dim in [
            UsageDimension::InputTokens,
            UsageDimension::OutputTokens,
            UsageDimension::TotalTokens,
        ] {
            assert_eq!(counter(&facts, dim), None, "{raw}");
        }
    }
    let body = r#"{"error":{"message":"private-error-marker"},"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}}"#;
    for (status, body) in [
        ("400 Bad Request", body),
        ("200 OK", body),
        (
            "200 OK",
            r#"{"choices":[{}],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}}"#,
        ),
    ] {
        let (url, server) =
            super::transport::test_support::server(status, body, false, std::time::Duration::ZERO);
        let s = remote_scheduler("groq", url, f.store.clone());
        assert!(run_remote(&s, structured_groq_request()).is_err());
        server.join().unwrap();
        assert_eq!(
            counter(&snapshot(&s, "groq"), UsageDimension::OutputTokens),
            None
        );
    }
}
#[test]
fn lr8a_fix_gemini_incomplete_keeps_only_valid_terminal_usage() {
    let f = HttpFixture::new(true);
    for (raw, expected) in [
        (
            r#"{"total_input_tokens":7,"total_output_tokens":3,"total_tokens":10}"#,
            Some(3),
        ),
        (
            r#"{"total_input_tokens":7,"total_output_tokens":-3,"total_tokens":10}"#,
            None,
        ),
        (
            r#"{"total_input_tokens":7,"total_output_tokens":3,"total_tokens":9}"#,
            None,
        ),
        ("null", None),
    ] {
        let body =
            sse("gemini", raw).replace("\"status\":\"completed\"", "\"status\":\"incomplete\"");
        let (url, server) = super::transport::test_support::server(
            "200 OK",
            &body,
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("gemini", url, f.store.clone());
        assert_eq!(
            run_remote(&s, remote_request("gemini")).unwrap_err(),
            SchedulerError::Provider(ProviderError::Incomplete)
        );
        server.join().unwrap();
        let facts = snapshot(&s, "gemini");
        assert_eq!(counter(&facts, UsageDimension::Requests), Some(1));
        assert_eq!(counter(&facts, UsageDimension::OutputTokens), expected);
        assert_eq!(
            counter(&facts, UsageDimension::InputTokens),
            expected.map(|_| 7)
        );
        assert_eq!(
            counter(&facts, UsageDimension::TotalTokens),
            expected.map(|_| 10)
        );
    }
}
#[test]
fn lr8a_fix_http_retry_date_is_factual_without_changing_error_or_cooldown() {
    let f = HttpFixture::new(true);
    for header in ["700000", "Fri, 01 Jan 2100 00:00:00 GMT"] {
        let (url, server) = super::transport::test_support::server(
            &format!("429 Too Many Requests\r\nRetry-After: {header}"),
            "{}",
            false,
            std::time::Duration::ZERO,
        );
        let s = remote_scheduler("groq", url, f.store.clone());
        assert_eq!(
            run_remote(&s, remote_request("groq")).unwrap_err(),
            SchedulerError::Provider(ProviderError::RateLimited {
                retry_after_ms: Some(604_800_000)
            })
        );
        server.join().unwrap();
        let expected = if header == "700000" {
            Timing::DelayMs(700_000_000)
        } else {
            Timing::UnixMs(4_102_444_800_000)
        };
        assert!(
            matches!(snapshot(&s, "groq").retry_hint, Fact::Known { value, .. } if value == expected)
        );
    }
}
