use super::*;
use crate::cognition::{
    provider::{Provider, ProviderFuture},
    types::{ProviderChunk, ProviderResponse, ProviderUsage, RetryPolicy},
};
use std::sync::Mutex;

struct Synthetic {
    error: Mutex<Option<ProviderError>>,
    seen: Mutex<Vec<ProviderTarget>>,
    partial: bool,
}
impl Synthetic {
    fn new(error: Option<ProviderError>) -> Arc<Self> {
        Arc::new(Self {
            error: Mutex::new(error),
            seen: Mutex::new(vec![]),
            partial: false,
        })
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}
impl Provider for Synthetic {
    fn execute<'a>(
        &'a self,
        request: &'a ProviderRequest,
        _: &'a AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(request.target.clone());
            if self.partial {
                chunk(ProviderChunk {
                    text: "partial".into(),
                })?;
            }
            if let Some(error) = self.error.lock().unwrap().clone() {
                return Err(error);
            }
            Ok(ProviderResponse {
                text: "ok".into(),
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 2,
                    output_tokens: 1,
                    ..Default::default()
                },
            })
        })
    }
}
fn scheduler(providers: &[(&str, u16, Arc<Synthetic>)]) -> Scheduler {
    let mut registry = ProviderRegistry::default();
    for (id, priority, provider) in providers.iter().rev() {
        registry
            .register(
                ProviderConfig {
                    id: (*id).into(),
                    priority: *priority,
                    enabled: true,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                simulated_transport(provider.clone()),
            )
            .unwrap();
    }
    Scheduler::new(registry)
}
fn route(
    db: &Database,
    ids: &[&str],
    selection: ProviderSelection,
    key: Option<&str>,
    bytes: usize,
) -> ProviderTaskRequest {
    let mut req = request(db, ids);
    req.selection = selection;
    req.affinity_key = key.map(str::to_owned);
    req.estimated_context_bytes = bytes;
    for (i, target) in req.targets.iter_mut().enumerate() {
        target.invocation.model = format!("model-{}", target.provider_id);
        target.invocation.thinking_level = Some(if i == 0 {
            super::super::policy::ThinkingLevel::High
        } else {
            super::super::policy::ThinkingLevel::Low
        });
        target.invocation.timeouts = Some(super::super::types::ProviderTimeouts {
            request_timeout_ms: 100 + i as u32,
            stream_idle_timeout_ms: 200 + i as u32,
        });
    }
    req
}
fn execute(
    s: &Scheduler,
    req: ProviderTaskRequest,
    calls: u32,
    retries: u32,
) -> (
    Result<super::super::types::TaskResult, SchedulerError>,
    Vec<SchedulerEvent>,
) {
    let mut events = vec![];
    let result = tauri::async_runtime::block_on(s.run_with_retry(
        req,
        budget(calls),
        RetryPolicy {
            enabled: retries > 0,
            max_retries: retries,
            initial_backoff_ms: 0,
        },
        &AtomicBool::new(false),
        &mut |e| {
            events.push(e);
            Ok(())
        },
    ));
    (result, events)
}
fn selected(events: &[SchedulerEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            SchedulerEvent::Selected { provider_id, .. } => Some(provider_id.as_str()),
            _ => None,
        })
        .collect()
}
fn limited() -> ProviderError {
    ProviderError::RateLimited {
        retry_after_ms: Some(60_000),
    }
}

#[test]
fn preferred_three_targets_follow_policy_order_and_keep_independent_invocations() {
    let (db, dir) = fixture();
    seed(&db);
    for failing in 0..=2 {
        let a = Synthetic::new((failing >= 1).then(limited));
        let b = Synthetic::new((failing >= 2).then(limited));
        let c = Synthetic::new(None);
        let s = scheduler(&[
            ("a", 32, a.clone()),
            ("b", 16, b.clone()),
            ("c", 0, c.clone()),
        ]);
        // Registry priority favors C; ordered targets still remain authoritative.
        let req = route(&db, &["a", "b", "c"], ProviderSelection::Preferred, None, 0);
        let configs = req.targets.clone();
        let (result, events) = execute(&s, req, 3, 0);
        let result = result.unwrap();
        assert_eq!(result.provider_id, ["a", "b", "c"][failing]);
        assert_eq!(selected(&events), &["a", "b", "c"][..=failing]);
        assert_eq!(result.usage.fallbacks, failing as u32);
        for (i, provider) in [a, b, c].iter().enumerate().take(failing + 1) {
            assert_eq!(*provider.seen.lock().unwrap(), vec![configs[i].clone()]);
        }
        assert!(events
            .iter()
            .filter_map(|e| {
                if let SchedulerEvent::Selected {
                    routing_reason,
                    score,
                    ..
                } = e
                {
                    Some((*routing_reason, *score))
                } else {
                    None
                }
            })
            .all(|item| item == ("preferred_order", None)));
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn preferred_cooldown_skips_call_and_events_and_budget_can_stop_before_third() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(Some(limited()));
    let b = Synthetic::new(Some(limited()));
    let c = Synthetic::new(None);
    let s = scheduler(&[
        ("a", 0, a.clone()),
        ("b", 1, b.clone()),
        ("c", 2, c.clone()),
    ]);
    let req = || route(&db, &["a", "b", "c"], ProviderSelection::Preferred, None, 0);
    let (result, events) = execute(&s, req(), 2, 0);
    assert!(result.is_err());
    assert_eq!(selected(&events), vec!["a", "b"]);
    assert_eq!(c.calls(), 0);
    let (result, events) = execute(&s, req(), 2, 0);
    assert_eq!(result.unwrap().provider_id, "c");
    assert_eq!(selected(&events), vec!["c"]);
    assert!(!events
        .iter()
        .any(|e| matches!(e, SchedulerEvent::Fallback { .. })));
    assert_eq!((a.calls(), b.calls(), c.calls()), (1, 1, 1));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn retries_spend_budget_before_advancing_chain() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(Some(ProviderError::Timeout));
    let b = Synthetic::new(None);
    let c = Synthetic::new(None);
    let s = scheduler(&[
        ("a", 0, a.clone()),
        ("b", 1, b.clone()),
        ("c", 2, c.clone()),
    ]);
    let (result, events) = execute(
        &s,
        route(&db, &["a", "b", "c"], ProviderSelection::Preferred, None, 0),
        2,
        1,
    );
    assert!(result.is_err());
    assert_eq!(selected(&events), vec!["a", "a"]);
    assert_eq!((a.calls(), b.calls(), c.calls()), (2, 0, 0));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn terminal_errors_and_partial_output_never_advance_chain() {
    let (db, dir) = fixture();
    seed(&db);
    for error in [
        ProviderError::Authentication,
        ProviderError::QuotaExceeded,
        ProviderError::InvalidRequest,
        ProviderError::Fatal,
        ProviderError::EventSinkClosed,
        ProviderError::Cancelled,
    ] {
        let a = Synthetic::new(Some(error));
        let b = Synthetic::new(None);
        let c = Synthetic::new(None);
        let s = scheduler(&[
            ("a", 0, a.clone()),
            ("b", 1, b.clone()),
            ("c", 2, c.clone()),
        ]);
        assert!(execute(
            &s,
            route(&db, &["a", "b", "c"], ProviderSelection::Preferred, None, 0),
            3,
            1
        )
        .0
        .is_err());
        assert_eq!((a.calls(), b.calls(), c.calls()), (1, 0, 0));
    }
    let a = Arc::new(Synthetic {
        error: Mutex::new(Some(limited())),
        seen: Mutex::new(vec![]),
        partial: true,
    });
    let b = Synthetic::new(None);
    let s = scheduler(&[("a", 0, a), ("b", 1, b.clone())]);
    let (result, events) = execute(
        &s,
        route(&db, &["a", "b"], ProviderSelection::Preferred, None, 0),
        3,
        1,
    );
    assert!(result.is_err());
    assert_eq!(b.calls(), 0);
    assert_eq!(selected(&events), vec!["a"]);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn auto_is_authorized_deterministic_and_policy_position_beats_registry_priority() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(None);
    let b = Synthetic::new(None);
    let unauthorized = Synthetic::new(None);
    let s = scheduler(&[
        ("a", u16::MAX, a.clone()),
        ("b", 0, b.clone()),
        ("unauthorized", 0, unauthorized.clone()),
    ]);
    for _ in 0..4 {
        let (result, events) = execute(
            &s,
            route(&db, &["a", "b"], ProviderSelection::Auto, None, usize::MAX),
            2,
            0,
        );
        assert_eq!(result.unwrap().provider_id, "a");
        assert!(matches!(
            &events[0],
            SchedulerEvent::Selected {
                routing_reason: "auto_allocator",
                score: Some(1530),
                ..
            }
        ));
    }
    assert_eq!(
        execute(
            &s,
            route(&db, &["b", "a"], ProviderSelection::Auto, None, 0),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "b"
    );
    assert_eq!(unauthorized.calls(), 0);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn auto_affinity_requires_real_cost_is_session_scoped_and_disappears_on_restart() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(None);
    let b = Synthetic::new(None);
    let providers = [("a", 1, a.clone()), ("b", 2, b.clone())];
    let s = scheduler(&providers);
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("conversation:A"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    assert_eq!(
        execute(
            &s,
            route(
                &db,
                &["a", "b"],
                ProviderSelection::Auto,
                Some("conversation:B"),
                4096
            ),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    // Continuity/switching now enter B2, whose profile weights decide the winner.
    let (result, events) = execute(
        &s,
        route(
            &db,
            &["a", "b"],
            ProviderSelection::Auto,
            Some("conversation:A"),
            4096,
        ),
        2,
        0,
    );
    assert_eq!(result.unwrap().provider_id, "b");
    assert!(matches!(
        &events[0],
        SchedulerEvent::Selected {
            routing_reason: "auto_allocator",
            score: Some(_),
            ..
        }
    ));
    assert_eq!(
        execute(
            &s,
            route(
                &db,
                &["a", "b"],
                ProviderSelection::Auto,
                Some("conversation:A"),
                0
            ),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    // A fresh Scheduler has no persisted continuity state.
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("conversation:B"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    let fresh = scheduler(&providers);
    assert_eq!(
        execute(
            &fresh,
            route(
                &db,
                &["a", "b"],
                ProviderSelection::Auto,
                Some("conversation:B"),
                usize::MAX
            ),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    // One byte maps to the smallest positive bounded B2 context signal.
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("tiny"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    assert_eq!(
        execute(
            &s,
            route(&db, &["a", "b"], ProviderSelection::Auto, Some("tiny"), 1),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "b"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn affinity_failure_cooldown_and_successful_alternative_update_the_winner() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(None);
    let b = Synthetic::new(None);
    let s = scheduler(&[("a", 1, a.clone()), ("b", 2, b.clone())]);
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("session"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    *b.error.lock().unwrap() = Some(limited());
    let (result, events) = execute(
        &s,
        route(
            &db,
            &["a", "b"],
            ProviderSelection::Auto,
            Some("session"),
            8192,
        ),
        2,
        0,
    );
    assert_eq!(result.unwrap().provider_id, "a");
    assert_eq!(selected(&events), vec!["b", "a"]);
    let (result, events) = execute(
        &s,
        route(
            &db,
            &["a", "b"],
            ProviderSelection::Auto,
            Some("session"),
            8192,
        ),
        2,
        0,
    );
    assert_eq!(result.unwrap().provider_id, "a");
    assert_eq!(selected(&events), vec!["a"]);
    assert_eq!(b.calls(), 2);
    // Use no cooldown error to show the alternate's successful affinity is refreshed.
    let a = Synthetic::new(None);
    let b = Synthetic::new(None);
    let s = scheduler(&[("a", 1, a.clone()), ("b", 2, b.clone())]);
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("session"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    *b.error.lock().unwrap() = Some(ProviderError::Timeout);
    assert_eq!(
        execute(
            &s,
            route(
                &db,
                &["a", "b"],
                ProviderSelection::Auto,
                Some("session"),
                8192
            ),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    *b.error.lock().unwrap() = None;
    let (result, events) = execute(
        &s,
        route(
            &db,
            &["b", "a"],
            ProviderSelection::Auto,
            Some("session"),
            8192,
        ),
        2,
        0,
    );
    assert_eq!(result.unwrap().provider_id, "a");
    assert_eq!(selected(&events), vec!["a"]);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn fixed_preferred_and_removed_targets_ignore_affinity_for_ordering() {
    let (db, dir) = fixture();
    seed(&db);
    let a = Synthetic::new(None);
    let b = Synthetic::new(None);
    let c = Synthetic::new(None);
    let s = scheduler(&[("a", 1, a), ("b", 2, b), ("c", 0, c)]);
    for selection in [
        ProviderSelection::Fixed("a".into()),
        ProviderSelection::Preferred,
    ] {
        execute(
            &s,
            route(
                &db,
                &["b"],
                ProviderSelection::Fixed("b".into()),
                Some("session"),
                0,
            ),
            1,
            0,
        )
        .0
        .unwrap();
        assert_eq!(
            execute(
                &s,
                route(&db, &["a", "b"], selection, Some("session"), usize::MAX),
                2,
                0
            )
            .0
            .unwrap()
            .provider_id,
            "a"
        );
    }
    execute(
        &s,
        route(
            &db,
            &["b"],
            ProviderSelection::Fixed("b".into()),
            Some("session"),
            0,
        ),
        1,
        0,
    )
    .0
    .unwrap();
    assert_eq!(
        execute(
            &s,
            route(
                &db,
                &["a", "c"],
                ProviderSelection::Auto,
                Some("session"),
                usize::MAX
            ),
            2,
            0
        )
        .0
        .unwrap()
        .provider_id,
        "a"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn auto_ties_respect_ordinal_and_priority_is_only_a_secondary_component() {
    let (db, dir) = fixture();
    seed(&db);
    for (priority, expected) in [(32, "a"), (0, "a")] {
        let s = scheduler(&[
            ("a", 32, Synthetic::new(None)),
            ("b", priority, Synthetic::new(None)),
        ]);
        execute(
            &s,
            route(
                &db,
                &["b"],
                ProviderSelection::Fixed("b".into()),
                Some("tie"),
                0,
            ),
            1,
            0,
        )
        .0
        .unwrap();
        let (result, events) = execute(
            &s,
            route(&db, &["a", "b"], ProviderSelection::Auto, Some("tie"), 0),
            2,
            0,
        );
        assert_eq!(result.unwrap().provider_id, expected);
        assert!(
            matches!(&events[0],SchedulerEvent::Selected{score:Some(score),..} if *score==1530)
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn hard_gates_prevent_calls_for_unknown_disabled_incompatible_or_invalid_targets() {
    let (db, dir) = fixture();
    seed(&db);
    for gate in ["unknown", "disabled", "capability", "invocation"] {
        let a = Synthetic::new(None);
        let b = Synthetic::new(None);
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "a".into(),
                    enabled: true,
                    priority: 0,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                a.clone(),
            )
            .unwrap();
        if gate != "unknown" {
            registry
                .register(
                    ProviderConfig {
                        id: "b".into(),
                        enabled: gate != "disabled",
                        priority: 0,
                        capabilities: if gate == "capability" {
                            ProviderCapabilities::default()
                        } else {
                            ProviderCapabilities::text_stream()
                        },
                    },
                    b.clone(),
                )
                .unwrap();
        }
        let s = Scheduler::new(registry);
        let mut req = route(&db, &["a", "b"], ProviderSelection::Auto, None, 0);
        if gate == "invocation" {
            req.targets[1].invocation.model = "".into();
        }
        let (result, events) = execute(&s, req, 2, 0);
        if matches!(gate, "unknown" | "invocation") {
            assert!(result.is_err());
            assert!(events.is_empty());
            assert_eq!((a.calls(), b.calls()), (0, 0));
        } else {
            assert_eq!(result.unwrap().provider_id, "a");
            assert_eq!(selected(&events), vec!["a"]);
            assert_eq!((a.calls(), b.calls()), (1, 0));
        }
    }
    fs::remove_dir_all(dir).unwrap();
}
