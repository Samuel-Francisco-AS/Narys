use super::{
    context::{ContextBuilder, ContextError, ContextRequest},
    mock::{MockProvider, MockScenario},
    registry::ProviderRegistry,
    scheduler::{Scheduler, SchedulerEvent},
    types::{
        ProviderCapabilities, ProviderConfig, ProviderError, ProviderInvocationConfig,
        ProviderRequest, ProviderSelection, ProviderTarget, ProviderTaskRequest, SchedulerError,
        TaskBudget,
    },
};
use crate::persistence::{
    conversation,
    database::Database,
    identity::{self, IdentityInput},
    memory::{self, MemoryInput},
};
use serde_json::json;
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn fixture() -> (Database, std::path::PathBuf) {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lr5-synthetic-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    (Database::for_test(dir.join("test.sqlite3")), dir)
}
fn identity(version: &str) -> IdentityInput {
    serde_json::from_value(json!({
    "version":version,"canonicalName":"Synthetic","presentation":"neutral","primaryLanguage":"pt-BR","concept":"test",
    "traits":{"curiosity":"high"},"behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
    "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
      "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
      "interactionPreferences":{"wantsRealDisagreement":true,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":true}},
    "memoryPolicy":{"retrieval":"selective","history":"versioned","continuity":"revisable","storePrivateChainOfThought":false},
    "provenance":"synthetic test","effectiveFrom":"2026-01-01"
  })).unwrap()
}
fn seed(db: &Database) {
    let mut conn = db.open().unwrap();
    let tx = conn.transaction().unwrap();
    identity::insert_version(&tx, &identity("v1")).unwrap();
    identity::insert_version(&tx, &identity("v2")).unwrap();
    for i in 0..12 {
        memory::import(
            &tx,
            &MemoryInput {
                import_key: format!("synthetic-{i}"),
                kind: "project".into(),
                domains: vec![if i == 10 { "other" } else { "projects" }.into()],
                state: if i == 11 { "historical" } else { "active" }.into(),
                title: format!("Synthetic {i}"),
                summary: format!("Synthetic private marker {i}"),
                content: None,
                retrieval_hint: None,
                source_context: None,
                importance: i as i64 % 10,
                confidence: "high".into(),
                event_date: Some("2026-01-01".into()),
            },
        )
        .unwrap();
    }
    tx.commit().unwrap();
}
fn context(db: &Database) -> super::types::ContextBundle {
    ContextBuilder::build(
        &db.open().unwrap(),
        ContextRequest {
            domain: Some("projects"),
            kind: None,
            min_importance: 0,
            memory_limit: 3,
            include_recent_conversation: false,
        },
    )
    .unwrap()
}
fn request(db: &Database, ids: &[&str]) -> ProviderTaskRequest {
    ProviderTaskRequest {
        mode: crate::cognition::types::InvocationMode::default(),
        input: "synthetic".into(),
        internal_system_instruction: None,
        history: vec![],
        context: Arc::new(context(db)),
        max_output_tokens: Some(30),
        selection: ProviderSelection::Auto,
        targets: ids
            .iter()
            .map(|id| ProviderTarget {
                provider_id: (*id).into(),
                invocation: ProviderInvocationConfig {
                    model: "mock".into(),
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

fn entry(
    id: &str,
    priority: u16,
    enabled: bool,
    caps: ProviderCapabilities,
    mock: Arc<MockProvider>,
    registry: &mut ProviderRegistry,
) {
    registry
        .register(
            ProviderConfig {
                id: id.into(),
                enabled,
                priority,
                capabilities: caps,
            },
            mock,
        )
        .unwrap();
}
fn budget(calls: u32) -> TaskBudget {
    TaskBudget {
        max_provider_calls: calls,
        max_output_tokens: Some(30),
    }
}

#[test]
fn context_builder_selects_current_active_filtered_bounded_and_optional_conversation() {
    let (db, dir) = fixture();
    let conn = db.open().unwrap();
    assert_eq!(
        ContextBuilder::build(
            &conn,
            ContextRequest {
                domain: None,
                kind: None,
                min_importance: 0,
                memory_limit: 5,
                include_recent_conversation: false
            }
        )
        .unwrap_err(),
        ContextError::IdentityUnavailable
    );
    drop(conn);
    seed(&db);
    let conn = db.open().unwrap();
    let bundle = ContextBuilder::build(
        &conn,
        ContextRequest {
            domain: Some("projects"),
            kind: Some("project"),
            min_importance: 5,
            memory_limit: 4,
            include_recent_conversation: true,
        },
    )
    .unwrap();
    assert_eq!(bundle.identity.version, "v2");
    assert!(bundle.relevant_memories.len() <= 4);
    assert!(bundle.relevant_memories.iter().all(|m| m.state == "active"
        && m.domains.contains(&"projects".into())
        && m.importance >= 5));
    assert!(bundle
        .relevant_memories
        .windows(2)
        .all(|w| w[0].importance >= w[1].importance));
    assert!(bundle.recent_messages.is_empty());
    assert_eq!(context(&db).relevant_memories.len(), 3);
    drop(conn);
    let mut conn = db.open().unwrap();
    let session_id = conversation::create_diagnostic(&mut conn).unwrap();
    for i in 0..10 {
        conn.execute(
            "INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user',?2)",
            rusqlite::params![session_id, format!("Synthetic message {i}")],
        )
        .unwrap();
    }
    let with_recent = ContextBuilder::build(
        &conn,
        ContextRequest {
            domain: None,
            kind: None,
            min_importance: 0,
            memory_limit: 99,
            include_recent_conversation: true,
        },
    )
    .unwrap();
    assert_eq!(with_recent.recent_messages.len(), 6);
    assert_eq!(
        with_recent.recent_messages[0].content,
        "Synthetic message 4"
    );
    assert_eq!(with_recent.relevant_memories.len(), 5);
    drop(conn);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn registry_filters_disabled_capability_and_orders_priority() {
    let mut registry = ProviderRegistry::default();
    let disabled = Arc::new(MockProvider::new(MockScenario::Normal));
    let no_stream = Arc::new(MockProvider::new(MockScenario::Normal));
    let preferred = Arc::new(MockProvider::new(MockScenario::Normal));
    entry(
        "disabled",
        0,
        false,
        ProviderCapabilities::text_stream(),
        disabled.clone(),
        &mut registry,
    );
    entry(
        "no-stream",
        1,
        true,
        ProviderCapabilities {
            text_generation: true,
            ..Default::default()
        },
        no_stream.clone(),
        &mut registry,
    );
    entry(
        "preferred",
        2,
        true,
        ProviderCapabilities::text_stream(),
        preferred.clone(),
        &mut registry,
    );
    assert!(registry.get("preferred").is_some());
    assert_eq!(
        registry
            .eligible(&ProviderCapabilities::text_stream())
            .iter()
            .map(|e| e.config.id.as_str())
            .collect::<Vec<_>>(),
        vec!["preferred"]
    );
    assert_eq!(disabled.calls(), 0);
    assert_eq!(no_stream.calls(), 0);
    let second = Arc::new(MockProvider::new(MockScenario::Normal));
    entry(
        "second",
        3,
        true,
        ProviderCapabilities::text_stream(),
        second,
        &mut registry,
    );
    assert_eq!(
        registry
            .eligible(&ProviderCapabilities::text_stream())
            .iter()
            .map(|e| e.config.id.as_str())
            .collect::<Vec<_>>(),
        vec!["preferred", "second"]
    );
}

#[test]
fn scheduler_fallback_cooldown_budget_retry_and_usage() {
    let (db, dir) = fixture();
    seed(&db);
    let primary = Arc::new(MockProvider::new(MockScenario::RateLimited));
    let fallback = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "mock-primary",
        1,
        true,
        ProviderCapabilities::text_stream(),
        primary.clone(),
        &mut registry,
    );
    entry(
        "mock-fallback",
        2,
        true,
        ProviderCapabilities::text_stream(),
        fallback.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let cancelled = AtomicBool::new(false);
    let mut events = vec![];
    let result = tauri::async_runtime::block_on(scheduler.run(
        request(&db, &["mock-primary", "mock-fallback"]),
        budget(3),
        &cancelled,
        &mut |e| {
            events.push(e);
            Ok(())
        },
    ));
    assert_eq!(result.unwrap().provider_id, "mock-fallback");
    assert_eq!(primary.calls(), 1);
    assert_eq!(fallback.calls(), 1);
    assert!(events.iter().any(|event| matches!(event,
      SchedulerEvent::Fallback { from, to, reason_code }
        if from == "mock-primary" && to == "mock-fallback" && *reason_code == "rate_limited"
    )));
    assert!(scheduler
        .status()
        .iter()
        .any(|s| s.id == "mock-primary" && s.cooldown_ms > 0));
    let next = tauri::async_runtime::block_on(scheduler.run(
        request(&db, &["mock-primary", "mock-fallback"]),
        budget(3),
        &cancelled,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(next.usage.provider_calls, 1);
    assert_eq!(primary.calls(), 1);
    assert_eq!(fallback.calls(), 2);
    let mut registry = ProviderRegistry::default();
    let limited = Arc::new(MockProvider::new(MockScenario::RateLimited));
    let untouched = Arc::new(MockProvider::new(MockScenario::Normal));
    entry(
        "a",
        1,
        true,
        ProviderCapabilities::text_stream(),
        limited.clone(),
        &mut registry,
    );
    entry(
        "b",
        2,
        true,
        ProviderCapabilities::text_stream(),
        untouched.clone(),
        &mut registry,
    );
    assert_eq!(
        tauri::async_runtime::block_on(Scheduler::new(registry).run(
            request(&db, &["a", "b"]),
            budget(1),
            &cancelled,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::RateLimited {
            retry_after_ms: Some(3_000)
        })
    );
    assert_eq!(untouched.calls(), 0);
    let mut registry = ProviderRegistry::default();
    let timeout = Arc::new(MockProvider::new(MockScenario::Timeout));
    entry(
        "timeout",
        1,
        true,
        ProviderCapabilities::text_stream(),
        timeout.clone(),
        &mut registry,
    );
    let retry = tauri::async_runtime::block_on(Scheduler::new(registry).run(
        request(&db, &["timeout"]),
        budget(2),
        &cancelled,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(retry.usage.provider_calls, 2);
    assert_eq!(retry.usage.retries, 1);
    assert_eq!(timeout.calls(), 2);
    let mut registry = ProviderRegistry::default();
    let transient = Arc::new(MockProvider::new(MockScenario::TransientThenSuccess));
    entry(
        "transient",
        1,
        true,
        ProviderCapabilities::text_stream(),
        transient.clone(),
        &mut registry,
    );
    let recovered = tauri::async_runtime::block_on(Scheduler::new(registry).run(
        request(&db, &["transient"]),
        budget(2),
        &cancelled,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(recovered.usage.retries, 1);
    assert_eq!(transient.calls(), 2);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preferred_groq_precedes_higher_priority_gemini_and_falls_back_to_it() {
    let (db, _) = fixture();
    seed(&db);
    for (groq_scenario, expected) in [
        (MockScenario::Normal, "groq"),
        (MockScenario::RateLimited, "gemini"),
    ] {
        let mut registry = ProviderRegistry::default();
        let gemini = Arc::new(MockProvider::new(MockScenario::Normal));
        let groq = Arc::new(MockProvider::new(groq_scenario));
        entry(
            "gemini",
            1,
            true,
            ProviderCapabilities::text_stream(),
            gemini.clone(),
            &mut registry,
        );
        entry(
            "groq",
            2,
            true,
            ProviderCapabilities::text_stream(),
            groq.clone(),
            &mut registry,
        );
        let scheduler = Scheduler::new(registry);
        let mut route = request(&db, &["gemini", "groq"]);
        route.selection = ProviderSelection::Preferred;
        route.targets.sort_by_key(|t| t.provider_id != "groq");
        assert_eq!(route.selection, ProviderSelection::Preferred);
        route
            .targets
            .retain(|target| target.provider_id == "groq" || target.provider_id == "gemini");
        let mut events = vec![];
        let result = tauri::async_runtime::block_on(scheduler.run(
            route,
            budget(2),
            &AtomicBool::new(false),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        ))
        .unwrap();
        assert_eq!(result.provider_id, expected, "events={events:?}");
        assert_eq!(groq.calls(), 1);
        assert_eq!(gemini.calls(), if expected == "gemini" { 1 } else { 0 });
        if expected == "gemini" {
            assert!(events.iter().any(|event| matches!(event, SchedulerEvent::Fallback { from, to, .. } if from == "groq" && to == "gemini")));
        }
    }
}

#[test]
fn mock_streaming_cancellation_and_error_classes() {
    let (db, dir) = fixture();
    seed(&db);
    let cancelled = Arc::new(AtomicBool::new(false));
    let mock = Arc::new(MockProvider::new(MockScenario::Streaming));
    let mut registry = ProviderRegistry::default();
    entry(
        "stream",
        1,
        true,
        ProviderCapabilities::text_stream(),
        mock,
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let mut chunks = vec![];
    let result = tauri::async_runtime::block_on(scheduler.run(
        request(&db, &["stream"]),
        budget(1),
        &cancelled,
        &mut |e| {
            if let SchedulerEvent::Chunk { text, .. } = e {
                chunks.push(text)
            }
            Ok(())
        },
    ))
    .unwrap();
    assert_eq!(chunks, vec!["Analisando ", "contexto ", "local..."]);
    assert_eq!(chunks.concat(), result.text);
    let signal = cancelled.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(145));
        signal.store(true, Ordering::Release)
    });
    let stopped = tauri::async_runtime::block_on(scheduler.run(
        request(&db, &["stream"]),
        budget(2),
        &cancelled,
        &mut |_| Ok(()),
    ));
    handle.join().unwrap();
    assert_eq!(stopped.unwrap_err(), SchedulerError::Cancelled);
    for (scenario, error) in [
        (MockScenario::QuotaExceeded, ProviderError::QuotaExceeded),
        (MockScenario::Fatal, ProviderError::Fatal),
    ] {
        let mut registry = ProviderRegistry::default();
        entry(
            "only",
            1,
            true,
            ProviderCapabilities::text_stream(),
            Arc::new(MockProvider::new(scenario)),
            &mut registry,
        );
        let signal = AtomicBool::new(false);
        assert_eq!(
            tauri::async_runtime::block_on(Scheduler::new(registry).run(
                request(&db, &["only"]),
                budget(2),
                &signal,
                &mut |_| Ok(())
            ))
            .unwrap_err(),
            SchedulerError::Provider(error)
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn disabled_provider_is_never_called_and_output_budget_is_respected() {
    let (db, dir) = fixture();
    seed(&db);
    let disabled = Arc::new(MockProvider::new(MockScenario::Normal));
    let enabled = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "disabled",
        0,
        false,
        ProviderCapabilities::text_stream(),
        disabled.clone(),
        &mut registry,
    );
    entry(
        "enabled",
        1,
        true,
        ProviderCapabilities::text_stream(),
        enabled.clone(),
        &mut registry,
    );
    let signal = AtomicBool::new(false);
    let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
        request(&db, &["enabled"]),
        TaskBudget {
            max_provider_calls: 1,
            max_output_tokens: Some(2),
        },
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "enabled");
    assert_eq!(result.usage.output_tokens, 2);
    assert_eq!(disabled.calls(), 0);
    assert_eq!(enabled.calls(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancellation_prevents_fallback_and_normal_is_deterministic() {
    let (db, dir) = fixture();
    seed(&db);
    let primary = Arc::new(MockProvider::new(MockScenario::Streaming));
    let fallback = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "primary",
        1,
        true,
        ProviderCapabilities::text_stream(),
        primary,
        &mut registry,
    );
    entry(
        "fallback",
        2,
        true,
        ProviderCapabilities::text_stream(),
        fallback.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let signal = Arc::new(AtomicBool::new(false));
    let signal_for_thread = signal.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(145));
        signal_for_thread.store(true, Ordering::Release);
    });
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(
            request(&db, &["primary", "fallback"]),
            budget(3),
            &signal,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Cancelled
    );
    handle.join().unwrap();
    assert_eq!(fallback.calls(), 0);
    let signal = AtomicBool::new(false);
    let mut registry = ProviderRegistry::default();
    entry(
        "normal",
        1,
        true,
        ProviderCapabilities::text_stream(),
        Arc::new(MockProvider::new(MockScenario::Normal)),
        &mut registry,
    );
    let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
        request(&db, &["normal"]),
        budget(1),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "normal");
    assert!(result.text.contains("3 memórias relevantes"));
    assert!(!result.text.contains("Synthetic private marker"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn event_sink_failure_stops_before_provider_during_stream_and_before_retry() {
    let (db, dir) = fixture();
    seed(&db);
    for (scenario, fail_at, expected_calls) in [
        (MockScenario::Normal, "selected", 0),
        (MockScenario::Streaming, "second_chunk", 1),
        (MockScenario::Timeout, "retry", 1),
    ] {
        let primary = Arc::new(MockProvider::new(scenario));
        let fallback = Arc::new(MockProvider::new(MockScenario::Normal));
        let mut registry = ProviderRegistry::default();
        entry(
            "primary",
            1,
            true,
            ProviderCapabilities::text_stream(),
            primary.clone(),
            &mut registry,
        );
        entry(
            "fallback",
            2,
            true,
            ProviderCapabilities::text_stream(),
            fallback.clone(),
            &mut registry,
        );
        let scheduler = Scheduler::new(registry);
        let signal = AtomicBool::new(false);
        let mut chunks = 0;
        let failure = tauri::async_runtime::block_on(scheduler.run(
            request(&db, &["primary", "fallback"]),
            budget(3),
            &signal,
            &mut |event| match event {
                SchedulerEvent::Selected { .. } if fail_at == "selected" => {
                    Err(SchedulerError::EventSinkClosed)
                }
                SchedulerEvent::Chunk { .. } if fail_at == "second_chunk" => {
                    chunks += 1;
                    if chunks == 2 {
                        Err(SchedulerError::EventSinkClosed)
                    } else {
                        Ok(())
                    }
                }
                SchedulerEvent::Retry { .. } if fail_at == "retry" => {
                    Err(SchedulerError::EventSinkClosed)
                }
                _ => Ok(()),
            },
        ));
        assert_eq!(failure.unwrap_err(), SchedulerError::EventSinkClosed);
        assert!(signal.load(Ordering::Acquire));
        assert_eq!(primary.calls(), expected_calls);
        assert_eq!(fallback.calls(), 0);
        if fail_at == "second_chunk" {
            assert_eq!(chunks, 2);
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn transient_mocks_repeat_per_task_with_same_runtime_and_cooldown_persists() {
    let (db, dir) = fixture();
    seed(&db);
    let runtime = super::CognitionRuntime::new();
    let signal = AtomicBool::new(false);
    for _ in 0..2 {
        let result = tauri::async_runtime::block_on(
            runtime
                .scheduler(super::DiagnosticScenario::TimeoutRetry)
                .run(
                    request(&db, &["mock-primary", "mock-fallback"]),
                    budget(3),
                    &signal,
                    &mut |_| Ok(()),
                ),
        )
        .unwrap();
        assert_eq!(result.usage.provider_calls, 2);
        assert_eq!(result.usage.retries, 1);
    }
    let mut registry = ProviderRegistry::default();
    entry(
        "transient",
        1,
        true,
        ProviderCapabilities::text_stream(),
        Arc::new(MockProvider::new(MockScenario::TransientThenSuccess)),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    for _ in 0..2 {
        let result = tauri::async_runtime::block_on(scheduler.run(
            request(&db, &["transient"]),
            budget(2),
            &signal,
            &mut |_| Ok(()),
        ))
        .unwrap();
        assert_eq!(result.usage.provider_calls, 2);
        assert_eq!(result.usage.retries, 1);
    }
    let a = tauri::async_runtime::block_on(
        runtime
            .scheduler(super::DiagnosticScenario::RateLimitFallback)
            .run(
                request(&db, &["mock-primary", "mock-fallback"]),
                budget(3),
                &signal,
                &mut |_| Ok(()),
            ),
    )
    .unwrap();
    let b = tauri::async_runtime::block_on(
        runtime
            .scheduler(super::DiagnosticScenario::RateLimitFallback)
            .run(
                request(&db, &["mock-primary", "mock-fallback"]),
                budget(3),
                &signal,
                &mut |_| Ok(()),
            ),
    )
    .unwrap();
    assert_eq!(a.provider_id, "mock-fallback");
    assert_eq!(b.usage.provider_calls, 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn partial_stream_failure_does_not_retry_or_fallback() {
    use super::{
        provider::{Provider, ProviderFuture},
        types::{ProviderChunk, ProviderResponse},
    };
    use std::sync::atomic::AtomicU32;
    struct Partial {
        calls: AtomicU32,
        error: ProviderError,
    }
    impl Provider for Partial {
        fn execute<'a>(
            &'a self,
            _request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                on_chunk(ProviderChunk {
                    text: "parcial".into(),
                })?;
                Err::<ProviderResponse, ProviderError>(self.error.clone())
            })
        }
    }
    let (db, dir) = fixture();
    seed(&db);
    for error in [
        ProviderError::Unavailable {
            retry_after_ms: None,
        },
        ProviderError::Timeout,
        ProviderError::RateLimited {
            retry_after_ms: None,
        },
        ProviderError::QuotaExceeded,
        ProviderError::Authentication,
        ProviderError::Incomplete,
    ] {
        let partial = Arc::new(Partial {
            calls: AtomicU32::new(0),
            error: error.clone(),
        });
        let fallback = Arc::new(MockProvider::new(MockScenario::Normal));
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "primary".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                partial.clone(),
            )
            .unwrap();
        entry(
            "fallback",
            2,
            true,
            ProviderCapabilities::text_stream(),
            fallback.clone(),
            &mut registry,
        );
        let signal = AtomicBool::new(false);
        let mut chunks = Vec::new();
        let mut retries = 0;
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            request(&db, &["primary", "fallback"]),
            budget(3),
            &signal,
            &mut |event| {
                match event {
                    SchedulerEvent::Chunk { text, .. } => chunks.push(text),
                    SchedulerEvent::Retry { .. } => retries += 1,
                    _ => {}
                }
                Ok(())
            },
        ));
        assert_eq!(result.unwrap_err(), SchedulerError::Provider(error));
        assert_eq!(chunks, vec!["parcial"]);
        assert_eq!(partial.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.calls(), 0);
        assert_eq!(retries, 0);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn scheduler_retries_only_transient_errors_before_a_chunk() {
    use super::{
        provider::{Provider, ProviderFuture},
        types::{ProviderChunk, ProviderResponse, ProviderUsage},
    };
    use std::sync::atomic::AtomicU32;
    struct Synthetic {
        error: ProviderError,
        calls: AtomicU32,
    }
    impl Provider for Synthetic {
        fn execute<'a>(
            &'a self,
            request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            _on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if request.attempt == 1 {
                    Err(self.error.clone())
                } else {
                    Ok(ProviderResponse {
                        text: "ok".into(),
                        usage: ProviderUsage {
                            calls: 1,
                            input_tokens: 1,
                            output_tokens: 1,
                            total_tokens: Some(2),
                            thought_tokens: None,
                            output_tokens_measured: true,
                        },
                    })
                }
            })
        }
    }
    let (db, dir) = fixture();
    seed(&db);
    for (error, retryable) in [
        (ProviderError::QuotaExceeded, false),
        (ProviderError::Authentication, false),
        (ProviderError::Protocol, false),
        (ProviderError::RequiresAction, false),
        (ProviderError::Fatal, false),
        (
            ProviderError::Unavailable {
                retry_after_ms: None,
            },
            true,
        ),
        (ProviderError::Timeout, true),
    ] {
        let provider = Arc::new(Synthetic {
            error: error.clone(),
            calls: AtomicU32::new(0),
        });
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "only".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                provider.clone(),
            )
            .unwrap();
        let signal = AtomicBool::new(false);
        let mut retries = 0;
        let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
            request(&db, &["only"]),
            budget(2),
            &signal,
            &mut |event| {
                if matches!(event, SchedulerEvent::Retry { .. }) {
                    retries += 1
                }
                Ok(())
            },
        ));
        if retryable {
            let result = result.unwrap();
            assert_eq!(result.usage.provider_calls, 2);
            assert_eq!(result.usage.retries, 1);
            assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
            assert_eq!(retries, 1);
        } else {
            assert_eq!(result.unwrap_err(), SchedulerError::Provider(error));
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            assert_eq!(retries, 0);
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn task_graph_conservatively_accounts_unknown_output_usage() {
    use super::{
        provider::{Provider, ProviderFuture},
        types::{ProviderChunk, ProviderResponse, ProviderUsage},
    };
    struct NoUsage;
    impl Provider for NoUsage {
        fn execute<'a>(
            &'a self,
            _request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            _on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async {
                Ok(ProviderResponse {
                    text: "resposta sem telemetria".into(),
                    usage: ProviderUsage::default(),
                })
            })
        }
    }
    let (db, dir) = fixture();
    seed(&db);
    let mut registry = ProviderRegistry::default();
    registry.register(
        ProviderConfig { id: "only".into(), enabled: true, priority: 1, capabilities: ProviderCapabilities::text_stream() },
        Arc::new(NoUsage),
    ).unwrap();
    let scheduler = Scheduler::new(registry);
    let result = tauri::async_runtime::block_on(scheduler.run_with_retry_conservative_output(
        request(&db, &["only"]),
        TaskBudget { max_provider_calls: 1, max_output_tokens: Some(17) },
        super::types::RetryPolicy { enabled: false, max_retries: 0, initial_backoff_ms: 0 },
        &AtomicBool::new(false),
        &mut |_| Ok(()),
    )).unwrap();
    assert_eq!(result.usage.output_tokens, 0);
    assert!(!result.usage.output_tokens_measured);
    assert_eq!(result.usage.output_tokens_accounted, 17);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_provider_and_unbounded_output() {
    let (db, dir) = fixture();
    seed(&db);
    let unwanted = Arc::new(MockProvider::new(MockScenario::Normal));
    let selected = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "first",
        1,
        true,
        ProviderCapabilities::text_stream(),
        unwanted.clone(),
        &mut registry,
    );
    entry(
        "selected",
        2,
        true,
        ProviderCapabilities::text_stream(),
        selected.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let signal = AtomicBool::new(false);
    let mut selected_request = request(&db, &["first", "selected"]);
    selected_request.selection = ProviderSelection::Fixed("selected".into());
    selected_request.max_output_tokens = None;
    let result = tauri::async_runtime::block_on(scheduler.run(
        selected_request,
        TaskBudget {
            max_provider_calls: 1,
            max_output_tokens: None,
        },
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "selected");
    assert!(result.usage.output_tokens > 0);
    assert_eq!(unwanted.calls(), 0);
    assert_eq!(selected.calls(), 1);
    let mut missing = request(&db, &["first", "selected"]);
    missing.selection = ProviderSelection::Fixed("missing".into());
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(missing, budget(1), &signal, &mut |_| Ok(())))
            .unwrap_err(),
        SchedulerError::NoProvider
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn retry_policy_obeys_call_budget_retry_limit_rate_limit_and_first_chunk() {
    use super::provider::{Provider, ProviderFuture};
    use super::types::{ProviderChunk, ProviderResponse, ProviderUsage, RetryPolicy};
    struct SequenceProvider {
        errors: Vec<Option<ProviderError>>,
        emit_before_error: bool,
        calls: std::sync::atomic::AtomicU32,
    }
    impl Provider for SequenceProvider {
        fn execute<'a>(
            &'a self,
            _request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async move {
                let index = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
                if self.emit_before_error {
                    on_chunk(ProviderChunk {
                        text: "partial".into(),
                    })?;
                }
                if let Some(error) = self.errors.get(index).cloned().flatten() {
                    return Err(error);
                }
                Ok(ProviderResponse {
                    text: "ok".into(),
                    usage: ProviderUsage {
                        calls: 1,
                        ..Default::default()
                    },
                })
            })
        }
    }
    let (db, dir) = fixture();
    seed(&db);
    let cases = [
        (
            2,
            1,
            vec![
                Some(ProviderError::Unavailable {
                    retry_after_ms: None,
                }),
                None,
            ],
            false,
            2,
            true,
        ),
        (
            1,
            10,
            vec![Some(ProviderError::Unavailable {
                retry_after_ms: None,
            })],
            false,
            1,
            false,
        ),
        (
            4,
            1,
            vec![
                Some(ProviderError::Unavailable {
                    retry_after_ms: None,
                }),
                Some(ProviderError::RateLimited {
                    retry_after_ms: Some(500),
                }),
            ],
            false,
            2,
            false,
        ),
        (
            4,
            1,
            vec![
                Some(ProviderError::Unavailable {
                    retry_after_ms: None,
                }),
                Some(ProviderError::Unavailable {
                    retry_after_ms: None,
                }),
            ],
            false,
            2,
            false,
        ),
        (
            4,
            3,
            vec![Some(ProviderError::RateLimited {
                retry_after_ms: Some(500),
            })],
            false,
            1,
            false,
        ),
        (
            4,
            3,
            vec![
                Some(ProviderError::Unavailable {
                    retry_after_ms: Some(30_000),
                }),
                None,
            ],
            false,
            1,
            false,
        ),
        (
            4,
            3,
            vec![Some(ProviderError::Unavailable {
                retry_after_ms: None,
            })],
            true,
            1,
            false,
        ),
        (
            4,
            1,
            vec![
                Some(ProviderError::Unavailable {
                    retry_after_ms: None,
                }),
                None,
            ],
            false,
            2,
            true,
        ),
    ];
    for (calls, retries, errors, chunk, expected_calls, success) in cases {
        let provider = Arc::new(SequenceProvider {
            errors,
            emit_before_error: chunk,
            calls: std::sync::atomic::AtomicU32::new(0),
        });
        let mut registry = ProviderRegistry::default();
        registry
            .register(
                ProviderConfig {
                    id: "gemini".into(),
                    enabled: true,
                    priority: 1,
                    capabilities: ProviderCapabilities::text_stream(),
                },
                provider.clone(),
            )
            .unwrap();
        let scheduler = Scheduler::new(registry);
        let signal = AtomicBool::new(false);
        let result = tauri::async_runtime::block_on(scheduler.run_with_retry(
            request(&db, &["gemini"]),
            budget(calls),
            RetryPolicy {
                enabled: true,
                max_retries: retries,
                initial_backoff_ms: 1,
            },
            &signal,
            &mut |_| Ok(()),
        ));
        assert_eq!(result.is_ok(), success);
        assert_eq!(provider.calls.load(Ordering::SeqCst), expected_calls);
        if expected_calls == 1
            && matches!(
                result,
                Err(SchedulerError::Provider(
                    ProviderError::RateLimited { .. }
                        | ProviderError::Unavailable {
                            retry_after_ms: Some(_)
                        }
                ))
            )
        {
            assert!(scheduler.status()[0].cooldown_ms > 0);
            assert_eq!(
                tauri::async_runtime::block_on(scheduler.run_with_retry(
                    request(&db, &["gemini"]),
                    budget(4),
                    RetryPolicy {
                        enabled: true,
                        max_retries: 3,
                        initial_backoff_ms: 1
                    },
                    &signal,
                    &mut |_| Ok(())
                ))
                .unwrap_err(),
                SchedulerError::NoProvider
            );
            assert_eq!(provider.calls.load(Ordering::SeqCst), expected_calls);
        }
    }
    assert_eq!(
        RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_backoff_ms: 1500
        }
        .backoff_ms(1),
        1500
    );
    assert_eq!(
        RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_backoff_ms: 1500
        }
        .backoff_ms(2),
        3000
    );
    assert_eq!(
        RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_backoff_ms: 1500
        }
        .backoff_ms(3),
        6000
    );
    let provider = Arc::new(SequenceProvider {
        errors: vec![
            Some(ProviderError::Unavailable {
                retry_after_ms: None,
            }),
            None,
        ],
        emit_before_error: false,
        calls: std::sync::atomic::AtomicU32::new(0),
    });
    let mut registry = ProviderRegistry::default();
    registry
        .register(
            ProviderConfig {
                id: "gemini".into(),
                enabled: true,
                priority: 1,
                capabilities: ProviderCapabilities::text_stream(),
            },
            provider.clone(),
        )
        .unwrap();
    let signal = AtomicBool::new(false);
    let disabled = tauri::async_runtime::block_on(Scheduler::new(registry).run_with_retry(
        request(&db, &["gemini"]),
        budget(4),
        RetryPolicy {
            enabled: false,
            max_retries: 3,
            initial_backoff_ms: 1,
        },
        &signal,
        &mut |_| Ok(()),
    ));
    assert_eq!(
        disabled.unwrap_err(),
        SchedulerError::Provider(ProviderError::Unavailable {
            retry_after_ms: None
        })
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn conservative_output_reserves_current_and_future_attempts_before_retry_events() {
    use super::provider::{Provider, ProviderFuture};
    use super::types::{ProviderChunk, ProviderResponse, ProviderUsage, RetryPolicy};

    struct ScriptedProvider {
        failures_before_success: usize,
        calls: std::sync::atomic::AtomicU32,
        limits: std::sync::Mutex<Vec<Option<u32>>>,
    }
    impl Provider for ScriptedProvider {
        fn execute<'a>(
            &'a self,
            request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            _on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async move {
                let index = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
                self.limits.lock().unwrap().push(request.max_output_tokens);
                if index < self.failures_before_success {
                    return Err(ProviderError::Timeout);
                }
                Ok(ProviderResponse {
                    text: "ok".into(),
                    usage: ProviderUsage { output_tokens: 7, output_tokens_measured: true, ..Default::default() },
                })
            })
        }
    }

    fn execute_case(max_calls: u32, failures: usize, output_limit: u32, max_retries: u32)
        -> (Result<super::types::TaskResult, SchedulerError>, u32, Vec<Option<u32>>, Vec<SchedulerEvent>) {
        let (db, dir) = fixture();
        seed(&db);
        let provider = Arc::new(ScriptedProvider {
            failures_before_success: failures,
            calls: std::sync::atomic::AtomicU32::new(0),
            limits: std::sync::Mutex::new(Vec::new()),
        });
        let mut registry = ProviderRegistry::default();
        registry.register(ProviderConfig {
            id: "groq".into(), enabled: true, priority: 1,
            capabilities: ProviderCapabilities::text_stream(),
        }, provider.clone()).unwrap();
        let scheduler = Scheduler::new(registry);
        let mut req = request(&db, &["groq"]);
        req.selection = ProviderSelection::Fixed("groq".into());
        req.max_output_tokens = Some(output_limit);
        let mut events = Vec::new();
        let result = tauri::async_runtime::block_on(scheduler.run_with_retry_conservative_output(
            req,
            TaskBudget { max_provider_calls: max_calls, max_output_tokens: Some(output_limit) },
            RetryPolicy { enabled: true, max_retries, initial_backoff_ms: 0 },
            &AtomicBool::new(false),
            &mut |event| { events.push(event); Ok(()) },
        ));
        let calls = provider.calls.load(Ordering::SeqCst);
        let limits = provider.limits.lock().unwrap().clone();
        fs::remove_dir_all(dir).unwrap();
        (result, calls, limits, events)
    }

    let (result, calls, limits, events) = execute_case(2, 1, 2048, 1);
    let result = result.expect("second attempt must have reserved output budget");
    assert_eq!(calls, 2);
    assert_eq!(result.usage.retries, 1);
    assert_eq!(limits, vec![Some(1024), Some(1024)]);
    assert!(result.usage.output_tokens_accounted <= 2048);
    assert_eq!(events.iter().filter(|event| matches!(event, SchedulerEvent::Retry { .. })).count(), 1);

    let (result, calls, limits, events) = execute_case(3, 2, 2048, 2);
    let result = result.expect("third attempt must have reserved output budget");
    assert_eq!(calls, 3);
    assert_eq!(result.usage.retries, 2);
    assert_eq!(limits, vec![Some(683), Some(683), Some(682)]);
    assert!(result.usage.output_tokens_accounted <= 2048);
    assert_eq!(events.iter().filter(|event| matches!(event, SchedulerEvent::Retry { .. })).count(), 2);

    // No retry event when its attempt has no output budget left to reserve.
    let (result, calls, limits, events) = execute_case(2, 2, 1, 2);
    assert!(matches!(result, Err(SchedulerError::Provider(ProviderError::Timeout))));
    assert_eq!(calls, 1);
    assert_eq!(limits, vec![Some(1)]);
    assert_eq!(events.iter().filter(|event| matches!(event, SchedulerEvent::Retry { .. })).count(), 0);

    // Exhausting provider calls must never start a third call.
    let (result, calls, limits, events) = execute_case(2, 3, 2048, 3);
    assert!(matches!(result, Err(SchedulerError::Provider(ProviderError::Timeout))));
    assert_eq!(calls, 2);
    assert_eq!(limits, vec![Some(1024), Some(1024)]);
    assert!(limits.iter().flatten().sum::<u32>() <= 2048);
    assert_eq!(events.iter().filter(|event| matches!(event, SchedulerEvent::Retry { .. })).count(), 1);
}

#[test]
fn selection_modes_cooldown_and_transient_fallback() {
    let (db, dir) = fixture();
    seed(&db);
    let first = Arc::new(MockProvider::new(MockScenario::Normal));
    let preferred = Arc::new(MockProvider::new(MockScenario::RateLimited));
    let alternative = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "first",
        1,
        true,
        ProviderCapabilities::text_stream(),
        first.clone(),
        &mut registry,
    );
    entry(
        "preferred",
        2,
        true,
        ProviderCapabilities::text_stream(),
        preferred.clone(),
        &mut registry,
    );
    entry(
        "alternative",
        3,
        true,
        ProviderCapabilities::text_stream(),
        alternative.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let signal = AtomicBool::new(false);
    let mut selected = request(&db, &["first", "preferred", "alternative"]);
    selected.selection = ProviderSelection::Preferred;
    selected
        .targets
        .sort_by_key(|t| t.provider_id != "preferred");
    let result = tauri::async_runtime::block_on(scheduler.run(
        selected,
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "first");
    assert_eq!(result.usage.providers_used, vec!["preferred", "first"]);
    assert_eq!(result.usage.fallbacks, 1);
    assert_eq!(
        (preferred.calls(), first.calls(), alternative.calls()),
        (1, 1, 0)
    );
    let mut selected = request(&db, &["first", "preferred", "alternative"]);
    selected.selection = ProviderSelection::Preferred;
    selected
        .targets
        .sort_by_key(|t| t.provider_id != "preferred");
    let result = tauri::async_runtime::block_on(scheduler.run(
        selected,
        budget(1),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "first");
    assert_eq!(preferred.calls(), 1);
    let result = tauri::async_runtime::block_on(scheduler.run(
        request(&db, &["first", "preferred", "alternative"]),
        budget(1),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "first");
    assert_eq!(preferred.calls(), 1);
    let mut fixed = request(&db, &["first", "preferred", "alternative"]);
    fixed.selection = ProviderSelection::Fixed("preferred".into());
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(fixed, budget(2), &signal, &mut |_| Ok(())))
            .unwrap_err(),
        SchedulerError::NoProvider
    );
    assert_eq!(preferred.calls(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fixed_transient_error_never_calls_alternative_and_preferred_healthy_wins() {
    let (db, dir) = fixture();
    seed(&db);
    let fixed = Arc::new(MockProvider::new(MockScenario::RateLimited));
    let other = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "other",
        1,
        true,
        ProviderCapabilities::text_stream(),
        other.clone(),
        &mut registry,
    );
    entry(
        "fixed",
        2,
        true,
        ProviderCapabilities::text_stream(),
        fixed.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let signal = AtomicBool::new(false);
    let mut selected = request(&db, &["other", "fixed"]);
    selected.selection = ProviderSelection::Fixed("fixed".into());
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(
            selected,
            budget(3),
            &signal,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::RateLimited {
            retry_after_ms: Some(3_000)
        })
    );
    assert_eq!((fixed.calls(), other.calls()), (1, 0));
    let healthy = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "other",
        1,
        true,
        ProviderCapabilities::text_stream(),
        other.clone(),
        &mut registry,
    );
    entry(
        "healthy",
        2,
        true,
        ProviderCapabilities::text_stream(),
        healthy.clone(),
        &mut registry,
    );
    let mut selected = request(&db, &["other", "healthy"]);
    selected.selection = ProviderSelection::Preferred;
    selected.targets.sort_by_key(|t| t.provider_id != "healthy");
    let result = tauri::async_runtime::block_on(Scheduler::new(registry).run(
        selected,
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "healthy");
    assert_eq!(healthy.calls(), 1);
    assert_eq!(other.calls(), 0);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unavailable_retry_after_falls_back_only_with_budget_and_auth_is_generic() {
    let (db, dir) = fixture();
    seed(&db);
    assert_eq!(ProviderError::Authentication.code(), "provider_auth_failed");
    let limited = Arc::new(MockProvider::new(MockScenario::UnavailableRetry));
    let other = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "limited",
        1,
        true,
        ProviderCapabilities::text_stream(),
        limited.clone(),
        &mut registry,
    );
    entry(
        "other",
        2,
        true,
        ProviderCapabilities::text_stream(),
        other.clone(),
        &mut registry,
    );
    let scheduler = Scheduler::new(registry);
    let signal = AtomicBool::new(false);
    let mut selected = request(&db, &["limited", "other"]);
    selected.selection = ProviderSelection::Preferred;
    selected.targets.sort_by_key(|t| t.provider_id != "limited");
    let result = tauri::async_runtime::block_on(scheduler.run(
        selected,
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "other");
    assert_eq!(result.usage.provider_calls, 2);
    assert!(scheduler
        .status()
        .iter()
        .any(|s| s.id == "limited" && s.cooldown_ms > 0));
    assert_eq!((limited.calls(), other.calls()), (1, 1));
    let fresh = Arc::new(MockProvider::new(MockScenario::UnavailableRetry));
    let untouched = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "limited",
        1,
        true,
        ProviderCapabilities::text_stream(),
        fresh.clone(),
        &mut registry,
    );
    entry(
        "other",
        2,
        true,
        ProviderCapabilities::text_stream(),
        untouched.clone(),
        &mut registry,
    );
    assert_eq!(
        tauri::async_runtime::block_on(Scheduler::new(registry).run(
            request(&db, &["limited", "other"]),
            budget(1),
            &signal,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::Unavailable {
            retry_after_ms: Some(3_000)
        })
    );
    assert_eq!((fresh.calls(), untouched.calls()), (1, 0));
    let terminal = Arc::new(MockProvider::new(MockScenario::QuotaExceeded));
    let untouched = Arc::new(MockProvider::new(MockScenario::Normal));
    let mut registry = ProviderRegistry::default();
    entry(
        "terminal",
        1,
        true,
        ProviderCapabilities::text_stream(),
        terminal.clone(),
        &mut registry,
    );
    entry(
        "other",
        2,
        true,
        ProviderCapabilities::text_stream(),
        untouched.clone(),
        &mut registry,
    );
    assert_eq!(
        tauri::async_runtime::block_on(Scheduler::new(registry).run(
            request(&db, &["terminal", "other"]),
            budget(2),
            &signal,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::QuotaExceeded)
    );
    assert_eq!((terminal.calls(), untouched.calls()), (1, 0));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn target_configuration_isolated_across_fixed_preferred_and_fallback() {
    use super::{
        provider::{Provider, ProviderFuture},
        types::{ProviderChunk, ProviderResponse, ProviderTimeouts, ProviderUsage},
    };
    use std::sync::Mutex;
    struct InspectingProvider {
        seen: Mutex<Vec<ProviderTarget>>,
        error: Option<ProviderError>,
        chunk_before_error: bool,
    }
    impl Provider for InspectingProvider {
        fn execute<'a>(
            &'a self,
            request: &'a ProviderRequest,
            _cancelled: &'a AtomicBool,
            on_chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
        ) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.seen.lock().unwrap().push(request.target.clone());
                if self.chunk_before_error {
                    on_chunk(ProviderChunk {
                        text: "partial".into(),
                    })?;
                }
                if let Some(error) = &self.error {
                    return Err(error.clone());
                }
                Ok(ProviderResponse {
                    text: "ok".into(),
                    usage: ProviderUsage {
                        calls: 1,
                        output_tokens: 1,
                        ..Default::default()
                    },
                })
            })
        }
    }
    fn target(
        id: &str,
        model: &str,
        thinking_level: Option<super::policy::ThinkingLevel>,
        timeout: u32,
    ) -> ProviderTarget {
        ProviderTarget {
            provider_id: id.into(),
            invocation: ProviderInvocationConfig {
                model: model.into(),
                thinking_level,
                timeouts: Some(ProviderTimeouts {
                    request_timeout_ms: timeout,
                    stream_idle_timeout_ms: timeout + 1,
                }),
            },
        }
    }
    fn make_scheduler(a: Arc<InspectingProvider>, b: Arc<InspectingProvider>) -> Scheduler {
        let mut registry = ProviderRegistry::default();
        for (id, priority, provider) in [("a", 1, a), ("b", 2, b)] {
            registry
                .register(
                    ProviderConfig {
                        id: id.into(),
                        enabled: true,
                        priority,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    provider,
                )
                .unwrap();
        }
        Scheduler::new(registry)
    }
    fn inspector(
        error: Option<ProviderError>,
        chunk_before_error: bool,
    ) -> Arc<InspectingProvider> {
        Arc::new(InspectingProvider {
            seen: Mutex::new(vec![]),
            error,
            chunk_before_error,
        })
    }
    let (db, dir) = fixture();
    seed(&db);
    let signal = AtomicBool::new(false);
    let a_config = target(
        "a",
        "model-a",
        Some(super::policy::ThinkingLevel::High),
        101,
    );
    let b_config = target("b", "model-b", None, 201);
    let configure = |selection| {
        let mut req = request(&db, &["a", "b"]);
        req.selection = selection;
        req.targets = if matches!(req.selection, ProviderSelection::Preferred) {
            vec![b_config.clone(), a_config.clone()]
        } else {
            vec![a_config.clone(), b_config.clone()]
        };
        req
    };

    let a = inspector(None, false);
    let b = inspector(None, false);
    let scheduler = make_scheduler(a.clone(), b.clone());
    let result = tauri::async_runtime::block_on(scheduler.run(
        configure(ProviderSelection::Fixed("a".into())),
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "a");
    assert_eq!(*a.seen.lock().unwrap(), vec![a_config.clone()]);
    assert!(b.seen.lock().unwrap().is_empty());

    let a = inspector(None, false);
    let b = inspector(None, false);
    let scheduler = make_scheduler(a.clone(), b.clone());
    let result = tauri::async_runtime::block_on(scheduler.run(
        configure(ProviderSelection::Preferred),
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "b");
    assert_eq!(*b.seen.lock().unwrap(), vec![b_config.clone()]);
    assert!(a.seen.lock().unwrap().is_empty());

    let a = inspector(
        Some(ProviderError::RateLimited {
            retry_after_ms: Some(3000),
        }),
        false,
    );
    let b = inspector(None, false);
    let scheduler = make_scheduler(a.clone(), b.clone());
    let result = tauri::async_runtime::block_on(scheduler.run(
        configure(ProviderSelection::Auto),
        budget(2),
        &signal,
        &mut |_| Ok(()),
    ))
    .unwrap();
    assert_eq!(result.provider_id, "b");
    assert_eq!(result.usage.providers_used, vec!["a", "b"]);
    assert_eq!(*a.seen.lock().unwrap(), vec![a_config.clone()]);
    assert_eq!(*b.seen.lock().unwrap(), vec![b_config.clone()]);
    assert_ne!(a_config.invocation.model, b_config.invocation.model);
    assert_ne!(
        a_config.invocation.thinking_level,
        b_config.invocation.thinking_level
    );
    assert_ne!(a_config.invocation.timeouts, b_config.invocation.timeouts);

    let a = inspector(
        Some(ProviderError::RateLimited {
            retry_after_ms: Some(3000),
        }),
        false,
    );
    let b = inspector(None, false);
    let scheduler = make_scheduler(a.clone(), b.clone());
    let mut missing = configure(ProviderSelection::Auto);
    missing.targets.pop();
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(missing, budget(2), &signal, &mut |_| Ok(())))
            .unwrap_err(),
        SchedulerError::Provider(ProviderError::RateLimited {
            retry_after_ms: Some(3000)
        })
    );
    assert_eq!(a.seen.lock().unwrap().len(), 1);
    assert!(b.seen.lock().unwrap().is_empty());
    let mut invalid = configure(ProviderSelection::Fixed("b".into()));
    invalid.targets[1].invocation.model = " ".into();
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(invalid, budget(1), &signal, &mut |_| Ok(())))
            .unwrap_err(),
        SchedulerError::InvalidTargetConfig
    );
    assert!(b.seen.lock().unwrap().is_empty());
    let mut duplicate = configure(ProviderSelection::Fixed("b".into()));
    duplicate.targets.push(b_config.clone());
    assert_eq!(
        tauri::async_runtime::block_on(
            scheduler.run(duplicate, budget(1), &signal, &mut |_| Ok(()))
        )
        .unwrap_err(),
        SchedulerError::InvalidTargetConfig
    );
    assert!(b.seen.lock().unwrap().is_empty());

    let a = inspector(
        Some(ProviderError::Unavailable {
            retry_after_ms: Some(3000),
        }),
        true,
    );
    let b = inspector(None, false);
    let scheduler = make_scheduler(a.clone(), b.clone());
    assert_eq!(
        tauri::async_runtime::block_on(scheduler.run(
            configure(ProviderSelection::Auto),
            budget(2),
            &signal,
            &mut |_| Ok(())
        ))
        .unwrap_err(),
        SchedulerError::Provider(ProviderError::Unavailable {
            retry_after_ms: Some(3000)
        })
    );
    assert!(b.seen.lock().unwrap().is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[path = "smart_routing_tests.rs"]
mod smart_routing;
