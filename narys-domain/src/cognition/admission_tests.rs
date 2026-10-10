use super::{
    admission::{AdmissionConfig, AdmissionController, AdmissionPermit, TrafficClass::*},
    provider::{Provider, ProviderFuture},
    registry::ProviderRegistry,
    scheduler::{Scheduler, SchedulerEvent},
    telemetry::{Fact, UsageDimension},
    types::*,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

fn config(cap: usize) -> AdmissionConfig {
    AdmissionConfig {
        max_concurrency_per_provider: cap,
        ..AdmissionConfig::default()
    }
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3), future)
        .await
        .expect("synchronization deadline")
}
fn controller(cap: usize, bypasses: u32) -> Arc<AdmissionController> {
    Arc::new(
        AdmissionController::new(
            ["a".into(), "b".into()],
            AdmissionConfig {
                max_priority_bypasses: bypasses,
                ..config(cap)
            },
        )
        .unwrap(),
    )
}
async fn hold(c: &AdmissionController, id: &str) -> AdmissionPermit {
    c.acquire(id, Background, &AtomicBool::new(false), &mut |_| Ok(()))
        .await
        .unwrap()
}
type Waiter = tokio::task::JoinHandle<Result<AdmissionPermit, SchedulerError>>;
async fn enqueue(
    c: Arc<AdmissionController>,
    class: super::admission::TrafficClass,
    cancel: Arc<AtomicBool>,
) -> Waiter {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        c.acquire("a", class, &cancel, &mut |depth| {
            tx.send(depth).unwrap();
            Ok(())
        })
        .await
    });
    bounded(rx.recv()).await.expect("queued");
    task
}
fn empty(c: &AdmissionController) {
    for s in c.snapshots() {
        assert_eq!((s.active_calls, s.queue_depth), (0, 0));
    }
}

#[tokio::test]
async fn cap_one_fifo_serializes_same_class() {
    let c = controller(1, 8);
    let running = hold(&c, "a").await;
    let one = enqueue(c.clone(), ForegroundTask, Arc::new(AtomicBool::new(false))).await;
    let two = enqueue(c.clone(), ForegroundTask, Arc::new(AtomicBool::new(false))).await;
    drop(running);
    let first = bounded(one).await.unwrap().unwrap();
    assert!(!two.is_finished());
    assert_eq!(c.snapshots()[0].active_calls, 1);
    drop(first);
    drop(bounded(two).await.unwrap().unwrap());
    empty(&c);
}
#[tokio::test]
async fn cap_two_admits_two_and_third_waits_without_preemption() {
    let c = controller(2, 8);
    let a = hold(&c, "a").await;
    let b = hold(&c, "a").await;
    let third = enqueue(
        c.clone(),
        ForegroundInteractive,
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    assert_eq!(
        (c.snapshots()[0].active_calls, c.snapshots()[0].queue_depth),
        (2, 1)
    );
    assert!(!third.is_finished());
    drop(a);
    let third = bounded(third).await.unwrap().unwrap();
    assert_eq!(c.snapshots()[0].active_calls, 2); // Background b remains admitted.
    drop(b);
    drop(third);
    empty(&c);
}
#[tokio::test]
async fn provider_states_are_independent() {
    let c = controller(1, 8);
    let a = hold(&c, "a").await;
    let queued = enqueue(c.clone(), Background, Arc::new(AtomicBool::new(false))).await;
    let b = bounded(hold(&c, "b")).await;
    assert_eq!(c.snapshots()[1].active_calls, 1);
    assert!(!queued.is_finished());
    drop(b);
    drop(a);
    drop(bounded(queued).await.unwrap().unwrap());
    empty(&c);
}
#[tokio::test]
async fn interactive_and_task_overtake_background_but_same_class_is_fifo() {
    let c = controller(1, 8);
    let running = hold(&c, "a").await;
    let bg = enqueue(c.clone(), Background, Arc::new(AtomicBool::new(false))).await;
    let task = enqueue(c.clone(), ForegroundTask, Arc::new(AtomicBool::new(false))).await;
    let chat = enqueue(
        c.clone(),
        ForegroundInteractive,
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    let chat2 = enqueue(
        c.clone(),
        ForegroundInteractive,
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    drop(running);
    let permit = bounded(chat).await.unwrap().unwrap();
    assert!(!chat2.is_finished() && !task.is_finished() && !bg.is_finished());
    drop(permit);
    drop(bounded(chat2).await.unwrap().unwrap());
    let permit = bounded(task).await.unwrap().unwrap();
    assert!(!bg.is_finished());
    drop(permit);
    drop(bounded(bg).await.unwrap().unwrap());
    empty(&c);
}
#[tokio::test]
async fn bounded_overtaking_protects_background_at_one_and_two_bypasses() {
    for limit in [1, 2] {
        let c = controller(1, limit);
        let running = hold(&c, "a").await;
        let bg = enqueue(c.clone(), Background, Arc::new(AtomicBool::new(false))).await;
        let mut high = std::collections::VecDeque::new();
        for _ in 0..limit + 1 {
            high.push_back(
                enqueue(
                    c.clone(),
                    ForegroundInteractive,
                    Arc::new(AtomicBool::new(false)),
                )
                .await,
            );
        }
        drop(running);
        for _ in 0..limit {
            drop(bounded(high.pop_front().unwrap()).await.unwrap().unwrap());
        }
        let protected = bounded(bg).await.unwrap().unwrap();
        assert!(!high.front().unwrap().is_finished());
        drop(protected);
        drop(bounded(high.pop_front().unwrap()).await.unwrap().unwrap());
        empty(&c);
    }
}
#[tokio::test]
async fn aborting_wait_or_running_permit_cleans_up() {
    let c = controller(1, 8);
    let running = hold(&c, "a").await;
    let queued = enqueue(c.clone(), Background, Arc::new(AtomicBool::new(false))).await;
    queued.abort();
    assert!(queued.await.err().unwrap().is_cancelled());
    assert_eq!(c.snapshots()[0].queue_depth, 0);
    drop(running);
    empty(&c);
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn({
        let c = c.clone();
        async move {
            let _permit = hold(&c, "a").await;
            tx.send(()).unwrap();
            std::future::pending::<()>().await;
        }
    });
    bounded(rx).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    empty(&c);
}

struct Invocation {
    provider: String,
    input: String,
    attempt: u32,
    finish: oneshot::Sender<Result<(), ProviderError>>,
}
struct Gate {
    id: String,
    tx: mpsc::UnboundedSender<Invocation>,
}
impl Provider for Gate {
    fn execute<'a>(
        &'a self,
        _: &'a ProviderRequest,
        _: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        panic!("observed invocation required")
    }
    fn execute_observed<'a>(
        &'a self,
        r: &'a ProviderRequest,
        cancel: &'a AtomicBool,
        _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        observation: &'a super::telemetry::InvocationObservation<'_>,
    ) -> ProviderFuture<'a> {
        assert!(
            !cancel.load(Ordering::Acquire),
            "provider invoked after cancellation won"
        );
        Box::pin(async move {
            let (finish, rx) = oneshot::channel();
            observation.started();
            self.tx
                .send(Invocation {
                    provider: self.id.clone(),
                    input: r.input.clone(),
                    attempt: r.attempt,
                    finish,
                })
                .unwrap();
            rx.await.unwrap()?;
            Ok(ProviderResponse {
                text: "private-output".into(),
                usage: ProviderUsage {
                    calls: 1,
                    output_tokens_measured: true,
                    ..ProviderUsage::default()
                },
            })
        })
    }
}
fn harness(cfg: AdmissionConfig) -> (Arc<Scheduler>, mpsc::UnboundedReceiver<Invocation>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut registry = ProviderRegistry::default();
    for id in ["a", "b"] {
        registry
            .register(
                ProviderConfig {
                    id: id.into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                Arc::new(Gate {
                    id: id.into(),
                    tx: tx.clone(),
                }),
            )
            .unwrap();
    }
    (
        Arc::new(Scheduler::with_admission_config(registry, cfg).unwrap()),
        rx,
    )
}
fn request(ids: &[&str], selection: ProviderSelection) -> ProviderTaskRequest {
    ProviderTaskRequest {
        allocation_policy: Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
            crate::cognitive_resources::provider_allocation_default(),
            None,
        )),
        traffic_class: ForegroundInteractive,
        mode: InvocationMode::default(),
        input: "private-prompt".into(),
        internal_system_instruction: Some("private-reasoning".into()),
        history: vec![],
        context: Arc::new(super::orchestrator::technical_context()),
        max_output_tokens: Some(100),
        selection,
        targets: ids
            .iter()
            .map(|id| ProviderTarget {
                provider_id: (*id).into(),
                invocation: ProviderInvocationConfig {
                    model: "local-model".into(),
                    thinking_level: None,
                    timeouts: Some(ProviderTimeouts {
                        request_timeout_ms: 1,
                        stream_idle_timeout_ms: 1,
                    }),
                },
            })
            .collect(),
        affinity_key: None,
        estimated_context_bytes: 0,
        required_capabilities: ProviderCapabilities::text_stream(),
    }
}
fn fixed(id: &str) -> ProviderTaskRequest {
    request(&[id], ProviderSelection::Fixed(id.into()))
}
fn budget() -> TaskBudget {
    TaskBudget {
        max_provider_calls: 4,
        max_output_tokens: Some(100),
    }
}
fn no_retry() -> RetryPolicy {
    RetryPolicy {
        enabled: false,
        max_retries: 0,
        initial_backoff_ms: 0,
    }
}
type Run = tokio::task::JoinHandle<Result<TaskResult, SchedulerError>>;
fn start(
    s: Arc<Scheduler>,
    r: ProviderTaskRequest,
    c: Arc<AtomicBool>,
    retry: RetryPolicy,
) -> (Run, mpsc::UnboundedReceiver<SchedulerEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        tokio::spawn(async move {
            s.run_with_retry(r, budget(), retry, &c, &mut |event| {
                let _ = tx.send(event);
                Ok(())
            })
            .await
        }),
        rx,
    )
}
async fn event(
    rx: &mut mpsc::UnboundedReceiver<SchedulerEvent>,
    pred: impl Fn(&SchedulerEvent) -> bool,
) -> SchedulerEvent {
    loop {
        let e = bounded(rx.recv()).await.expect("event channel");
        if pred(&e) {
            return e;
        }
    }
}
fn clean(s: &Scheduler) {
    empty(&s.admission);
}
fn factual_requests(s: &Scheduler, id: &str) -> u64 {
    let snapshots = s.telemetry_snapshot();
    match snapshots
        .iter()
        .find(|s| s.provider_id == id)
        .unwrap()
        .usage[&UsageDimension::Requests]
        .observed
    {
        Fact::Known { value, .. } => value,
        _ => panic!("requests must be factual"),
    }
}

#[tokio::test]
async fn scheduler_concurrency_one_two_and_third_request() {
    for cap in [1, 2] {
        let (s, mut calls) = harness(config(cap));
        let mut running = vec![];
        let mut invocations = vec![];
        for _ in 0..cap {
            running.push(
                start(
                    s.clone(),
                    fixed("a"),
                    Arc::new(AtomicBool::new(false)),
                    no_retry(),
                )
                .0,
            );
            invocations.push(bounded(calls.recv()).await.unwrap());
        }
        let (queued, mut events) = start(
            s.clone(),
            fixed("a"),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
        assert!(calls.try_recv().is_err());
        assert_eq!(s.admission_snapshot()[0].active_calls, cap);
        invocations.remove(0).finish.send(Ok(())).unwrap();
        bounded(running.remove(0)).await.unwrap().unwrap();
        let next = bounded(calls.recv()).await.unwrap();
        next.finish.send(Ok(())).unwrap();
        bounded(queued).await.unwrap().unwrap();
        for i in invocations {
            i.finish.send(Ok(())).unwrap();
        }
        for r in running {
            bounded(r).await.unwrap().unwrap();
        }
        clean(&s);
        assert_eq!(s.admission_snapshot()[0].total_admissions, cap as u64 + 1);
        assert_eq!(s.admission_snapshot()[0].queue_delay_samples, 1);
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_cancellation_and_release_race_never_invoke_or_leak() {
    let (s, mut calls) = harness(config(1));
    for n in 0..40 {
        let held = hold(&s.admission, "a").await;
        let cancel = Arc::new(AtomicBool::new(false));
        let (run, mut events) = start(s.clone(), fixed("a"), cancel.clone(), no_retry());
        event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
        cancel.store(true, Ordering::Release);
        if n % 2 == 0 {
            drop(held);
            assert_eq!(
                bounded(run).await.unwrap().unwrap_err(),
                SchedulerError::Cancelled
            );
        } else {
            assert_eq!(
                bounded(run).await.unwrap().unwrap_err(),
                SchedulerError::Cancelled
            );
            drop(held);
        }
        assert!(calls.try_recv().is_err());
        clean(&s);
    }
    assert_eq!(factual_requests(&s, "a"), 0);
    assert_eq!(s.admission_snapshot()[0].total_waited, 40);
}
#[tokio::test]
async fn cancellation_from_admitted_event_prevents_invocation() {
    let (s, mut calls) = harness(config(1));
    let c = AtomicBool::new(false);
    let result = s
        .run(fixed("a"), budget(), &c, &mut |e| {
            if matches!(e, SchedulerEvent::Admitted { .. }) {
                c.store(true, Ordering::Release);
            }
            Ok(())
        })
        .await;
    assert_eq!(result.unwrap_err(), SchedulerError::Cancelled);
    assert!(calls.try_recv().is_err());
    clean(&s);
    assert_eq!(factual_requests(&s, "a"), 0);
}
#[tokio::test]
async fn queue_full_is_local_terminal_without_retry_fallback_or_cooldown() {
    let (s, mut calls) = harness(AdmissionConfig {
        queue_capacity_per_provider: 1,
        ..config(1)
    });
    let held = hold(&s.admission, "a").await;
    let c = Arc::new(AtomicBool::new(false));
    let (waiting, mut e) = start(s.clone(), fixed("a"), c.clone(), no_retry());
    event(&mut e, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    let mut events = vec![];
    let result = s
        .run(
            request(&["a", "b"], ProviderSelection::Preferred),
            budget(),
            &AtomicBool::new(false),
            &mut |e| {
                events.push(e);
                Ok(())
            },
        )
        .await;
    assert_eq!(result.unwrap_err(), SchedulerError::AdmissionQueueFull);
    assert!(!events.iter().any(|e| matches!(
        e,
        SchedulerEvent::Retry { .. }
            | SchedulerEvent::Fallback { .. }
            | SchedulerEvent::Admitted { .. }
    )));
    assert!(calls.try_recv().is_err());
    assert_eq!(s.admission_snapshot()[0].queue_full_count, 1);
    assert!(s.status().iter().all(|s| s.cooldown_ms == 0));
    assert_eq!(factual_requests(&s, "a"), 0);
    c.store(true, Ordering::Release);
    assert_eq!(
        bounded(waiting).await.unwrap().unwrap_err(),
        SchedulerError::Cancelled
    );
    drop(held);
    clean(&s);
}
#[tokio::test]
async fn queue_timeout_is_separate_terminal_and_does_not_consume_request_timeout() {
    let (s, mut calls) = harness(AdmissionConfig {
        queue_timeout_ms: 120,
        ..config(1)
    });
    let held = hold(&s.admission, "a").await;
    let (run, mut events) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        RetryPolicy {
            enabled: true,
            max_retries: 2,
            initial_backoff_ms: 0,
        },
    );
    event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    assert_eq!(
        bounded(run).await.unwrap().unwrap_err(),
        SchedulerError::AdmissionTimeout
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(s.admission_snapshot()[0].queue_timeout_count, 1);
    assert_eq!(s.admission_snapshot()[0].queue_depth, 0);
    assert_eq!(factual_requests(&s, "a"), 0);
    while let Ok(e) = events.try_recv() {
        assert!(!matches!(
            e,
            SchedulerEvent::Retry { .. } | SchedulerEvent::Fallback { .. }
        ));
    }
    assert!(s.status().iter().all(|s| s.cooldown_ms == 0));
    drop(held);
    clean(&s);
    // Waiting beyond the 1ms provider timeouts can still be admitted successfully.
    let held = hold(&s.admission, "a").await;
    let (run, mut events) = start(
        s.clone(),
        fixed("a"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(held);
    bounded(calls.recv())
        .await
        .unwrap()
        .finish
        .send(Ok(()))
        .unwrap();
    bounded(run).await.unwrap().unwrap();
    clean(&s);
}
#[tokio::test]
async fn event_sink_closed_at_queued_or_admitted_never_invokes_provider() {
    for stage in ["queued", "admitted"] {
        let (s, mut calls) = harness(config(1));
        let held = if stage == "queued" {
            Some(hold(&s.admission, "a").await)
        } else {
            None
        };
        let result = s
            .run(fixed("a"), budget(), &AtomicBool::new(false), &mut |e| {
                // Reading snapshot in callback also proves callback is outside internal locks.
                s.admission_snapshot();
                if (stage == "queued" && matches!(e, SchedulerEvent::Queued { .. }))
                    || (stage == "admitted" && matches!(e, SchedulerEvent::Admitted { .. }))
                {
                    Err(SchedulerError::EventSinkClosed)
                } else {
                    Ok(())
                }
            })
            .await;
        assert_eq!(result.unwrap_err(), SchedulerError::EventSinkClosed);
        assert!(calls.try_recv().is_err());
        drop(held);
        clean(&s);
        assert_eq!(factual_requests(&s, "a"), 0);
    }
}
#[tokio::test]
async fn all_provider_terminal_paths_release_permit() {
    for error in [
        ProviderError::Authentication,
        ProviderError::Timeout,
        ProviderError::Protocol,
        ProviderError::Fatal,
        ProviderError::OutputLimitExceeded,
        ProviderError::InvalidRequest,
        ProviderError::EventSinkClosed,
        ProviderError::Cancelled,
        ProviderError::RateLimited {
            retry_after_ms: Some(1),
        },
        ProviderError::Unavailable {
            retry_after_ms: None,
        },
        ProviderError::QuotaExceeded,
        ProviderError::RemoteCancelled,
        ProviderError::Incomplete,
        ProviderError::RequiresAction,
        ProviderError::UnsupportedMode,
    ] {
        let (s, mut calls) = harness(config(1));
        let (run, _events) = start(
            s.clone(),
            fixed("a"),
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        bounded(calls.recv())
            .await
            .unwrap()
            .finish
            .send(Err(error))
            .unwrap();
        assert!(bounded(run).await.unwrap().is_err());
        clean(&s);
    }
}
#[tokio::test]
async fn retry_reacquires_and_releases_permit_before_backoff() {
    let (s, mut calls) = harness(config(1));
    let (run, mut events) = start(
        s.clone(),
        fixed("a"),
        Arc::new(AtomicBool::new(false)),
        RetryPolicy {
            enabled: true,
            max_retries: 1,
            initial_backoff_ms: 150,
        },
    );
    let first = bounded(calls.recv()).await.unwrap();
    assert_eq!(first.attempt, 1);
    first.finish.send(Err(ProviderError::Timeout)).unwrap();
    event(&mut events, |e| matches!(e, SchedulerEvent::Retry { .. })).await;
    clean(&s);
    // Occupy the released slot during backoff; retry must queue and acquire afresh.
    let held = hold(&s.admission, "a").await;
    event(&mut events, |e| matches!(e, SchedulerEvent::Queued { .. })).await;
    assert!(calls.try_recv().is_err());
    drop(held);
    let second = bounded(calls.recv()).await.unwrap();
    assert_eq!(second.attempt, 2);
    second.finish.send(Ok(())).unwrap();
    let result = bounded(run).await.unwrap().unwrap();
    assert_eq!(result.usage.retries, 1);
    clean(&s);
    assert_eq!(factual_requests(&s, "a"), 2);
}
#[tokio::test]
async fn fallback_releases_source_before_acquiring_destination() {
    let (s, mut calls) = harness(config(1));
    let held = hold(&s.admission, "b").await;
    let (run, mut events) = start(
        s.clone(),
        request(&["a", "b"], ProviderSelection::Preferred),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    let first = bounded(calls.recv()).await.unwrap();
    assert_eq!(first.provider, "a");
    first
        .finish
        .send(Err(ProviderError::Unavailable {
            retry_after_ms: None,
        }))
        .unwrap();
    event(
        &mut events,
        |e| matches!(e,SchedulerEvent::Queued{provider_id,..} if provider_id=="b"),
    )
    .await;
    assert_eq!(s.admission_snapshot()[0].active_calls, 0);
    assert_eq!(s.admission_snapshot()[1].active_calls, 1);
    let source = hold(&s.admission, "a").await;
    drop(source);
    drop(held);
    let next = bounded(calls.recv()).await.unwrap();
    assert_eq!(next.provider, "b");
    next.finish.send(Ok(())).unwrap();
    assert_eq!(bounded(run).await.unwrap().unwrap().usage.fallbacks, 1);
    clean(&s);
}
#[tokio::test]
async fn local_saturation_and_quota_zero_preserve_ranking_selection_score_and_affinity() {
    for selection in [
        ProviderSelection::Fixed("a".into()),
        ProviderSelection::Preferred,
        ProviderSelection::Auto,
    ] {
        let (s, mut calls) = harness(config(1));
        let mut r = request(&["a", "b"], selection.clone());
        r.affinity_key = Some("session".into());
        r.estimated_context_bytes = 1024 * 10;
        // Establish b affinity on the same Scheduler.
        let mut warm = fixed("b");
        warm.affinity_key = Some("session".into());
        let (warm, _events) = start(
            s.clone(),
            warm,
            Arc::new(AtomicBool::new(false)),
            no_retry(),
        );
        bounded(calls.recv())
            .await
            .unwrap()
            .finish
            .send(Ok(()))
            .unwrap();
        bounded(warm).await.unwrap().unwrap();
        let expected = if selection == ProviderSelection::Auto {
            "b"
        } else {
            "a"
        };
        let _ranking = s
            .ranked_provider_ids(
                &selection,
                &r.targets,
                &r.required_capabilities,
                &r.mode,
                Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                    crate::cognitive_resources::provider_allocation_default(),
                    None,
                ))
                .as_ref(),
            )
            .unwrap();
        let held = hold(&s.admission, expected).await;
        s.telemetry.observe_quota(
            expected,
            super::telemetry::QuotaScope::Provider,
            super::telemetry::QuotaDimension::RequestsPerMinute,
            Some(10),
            Some(0),
            None,
            super::telemetry::Provenance::ProviderHeader,
        );
        if selection == ProviderSelection::Auto {
            let (run, mut events) =
                start(s.clone(), r, Arc::new(AtomicBool::new(false)), no_retry());
            let chosen = event(&mut events, |e| {
                matches!(e, SchedulerEvent::Selected { .. })
            })
            .await;
            assert!(
                matches!(chosen, SchedulerEvent::Selected { provider_id, routing_reason: "auto_allocator", .. } if provider_id == "a")
            );
            let call = bounded(calls.recv()).await.unwrap();
            assert_eq!(call.provider, "a");
            call.finish.send(Ok(())).unwrap();
            assert_eq!(bounded(run).await.unwrap().unwrap().provider_id, "a");
            let targets = request(&["a", "b"], selection.clone());
            assert_eq!(
                s.ranked_provider_ids(
                    &selection,
                    &targets.targets,
                    &targets.required_capabilities,
                    &targets.mode,
                    Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                        crate::cognitive_resources::provider_allocation_default(),
                        None
                    ))
                    .as_ref(),
                )
                .unwrap(),
                vec!["a"]
            );
            drop(held);
            clean(&s);
            continue;
        }
        let (run, mut events) = start(s.clone(), r, Arc::new(AtomicBool::new(false)), no_retry());
        let selected = event(&mut events, |e| {
            matches!(e, SchedulerEvent::Selected { .. })
        })
        .await;
        if let SchedulerEvent::Selected {
            provider_id,
            score,
            routing_reason,
            ..
        } = selected
        {
            assert_eq!(provider_id, expected);
            assert_eq!(score, None);
            assert!(matches!(routing_reason, "fixed" | "preferred_order"));
        }
        assert_eq!(
            bounded(run).await.unwrap().unwrap_err(),
            SchedulerError::RateCapacityExceeded
        );
        assert_eq!(
            s.admission_snapshot()
                .iter()
                .map(|s| s.queue_depth)
                .sum::<usize>(),
            0
        );
        assert!(calls.try_recv().is_err());
        let targets = request(&["a", "b"], selection.clone());
        assert_eq!(
            _ranking,
            s.ranked_provider_ids(
                &selection,
                &targets.targets,
                &targets.required_capabilities,
                &targets.mode,
                Some(crate::cognitive_resources::AllocationRuntimePolicy::new(
                    crate::cognitive_resources::provider_allocation_default(),
                    None
                ))
                .as_ref(),
            )
            .unwrap()
        );
        assert!(s.status().iter().all(|s| s.cooldown_ms == 0));
        drop(held);
        assert!(calls.try_recv().is_err());
        clean(&s);
    }
}
#[tokio::test]
async fn admission_snapshot_and_events_contain_only_local_metadata() {
    let (s, mut calls) = harness(config(1));
    let (run, mut events) = start(
        s.clone(),
        fixed("a"),
        Arc::new(AtomicBool::new(false)),
        no_retry(),
    );
    event(&mut events, |e| {
        matches!(e, SchedulerEvent::Admitted { .. })
    })
    .await;
    let invocation = bounded(calls.recv()).await.unwrap();
    assert_eq!(invocation.input, "private-prompt");
    let json = serde_json::to_string(&s.admission_snapshot()).unwrap();
    for private in [
        "private-prompt",
        "private-output",
        "private-reasoning",
        "credential",
        "api_key",
        "headers",
        "context",
        "local-model",
    ] {
        assert!(!json.contains(private));
    }
    invocation.finish.send(Ok(())).unwrap();
    bounded(run).await.unwrap().unwrap();
    clean(&s);
}

#[tokio::test]
async fn preflight_authentication_releases_admission_without_factual_request() {
    struct Preflight;
    impl Provider for Preflight {
        fn execute<'a>(
            &'a self,
            _: &'a ProviderRequest,
            _: &'a AtomicBool,
            _: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async { Err(ProviderError::Authentication) })
        }
    }
    let mut providers = ProviderRegistry::default();
    providers
        .register(
            ProviderConfig {
                id: "a".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
            },
            Arc::new(Preflight),
        )
        .unwrap();
    let s = Scheduler::with_admission_config(providers, config(1)).unwrap();
    for _ in 0..2 {
        let result = s
            .run(fixed("a"), budget(), &AtomicBool::new(false), &mut |_| {
                Ok(())
            })
            .await;
        assert_eq!(
            result.unwrap_err(),
            SchedulerError::Provider(ProviderError::Authentication)
        );
        clean(&s);
    }
    assert_eq!(s.admission_snapshot()[0].total_admissions, 2);
    assert_eq!(factual_requests(&s, "a"), 0);
}
