//! Fixed opt-in native workload. No arbitrary IPC, execution authority or real
//! provider. Product Conversation/Summary still use the loopback PERF-1C fixture.
use crate::{
    agents::{
        trace::{AgentTraceObservation, AgentTraceSink},
        types::AgentEvent,
    },
    cognition::{
        policy::CognitiveRole,
        provider::{Provider, ProviderFuture},
        registry::ProviderRegistry,
        scheduler::Scheduler,
        types::*,
    },
    luna::{
        runtime::{ActiveTask, TaskRegistry},
        task::{TaskEventKind, TaskId, TaskState},
    },
    operational_trace::{adapters::*, *},
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};
static CALLS: AtomicU64 = AtomicU64::new(0);
static CHUNKS: AtomicU64 = AtomicU64::new(0);
static LAST_TASK: AtomicU64 = AtomicU64::new(0);
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct FakeProvider(&'static str);
impl Provider for FakeProvider {
    fn execute<'a>(
        &'a self,
        r: &'a ProviderRequest,
        cancel: &'a std::sync::atomic::AtomicBool,
        chunk: &'a mut (dyn FnMut(ProviderChunk) -> Result<(), ProviderError> + Send),
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            CALLS.fetch_add(1, Ordering::Relaxed);
            if self.0 == "fixture_p" {
                return Err(if r.attempt == 1 {
                    ProviderError::Timeout
                } else {
                    ProviderError::Unavailable {
                        retry_after_ms: None,
                    }
                });
            }
            for _ in 0..600 {
                if cancel.load(Ordering::Acquire) {
                    return Err(ProviderError::Cancelled);
                }
                chunk(ProviderChunk {
                    text: if r.input == "internal" {
                        "WORKER-INTERNAL-PRIVATE-MARKER"
                    } else {
                        "NATIVE-ALLOWED-CONVERSATION 🦀\n"
                    }
                    .into(),
                })?;
                CHUNKS.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(4)).await;
            }
            Ok(ProviderResponse {
                text: String::new(),
                usage: ProviderUsage {
                    calls: 1,
                    input_tokens: 11,
                    output_tokens: 13,
                    total_tokens: Some(24),
                    thought_tokens: None,
                    output_tokens_measured: true,
                },
            })
        })
    }
}
fn request(internal: bool) -> ProviderTaskRequest {
    ProviderTaskRequest {
        allocation_policy: None,
        traffic_class: crate::cognition::admission::TrafficClass::ForegroundTask,
        mode: InvocationMode::default(),
        input: if internal {
            "internal"
        } else {
            "USER-INPUT-PRIVATE-MARKER"
        }
        .into(),
        internal_system_instruction: None,
        history: vec![],
        context: Arc::new(crate::cognition::orchestrator::technical_context()),
        max_output_tokens: Some(100),
        selection: ProviderSelection::Preferred,
        targets: ["fixture_p", "fixture_q"]
            .into_iter()
            .map(|id| ProviderTarget {
                provider_id: id.into(),
                invocation: ProviderInvocationConfig {
                    model: "fixture_model".into(),
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
fn workload(app: &AppHandle) -> Result<(), String> {
    let registry = app.state::<Arc<TaskRegistry>>().inner().clone();
    if ACTIVE.swap(true, Ordering::AcqRel) {
        return Err("fixture already running".into());
    }
    let (task, cancel) = match registry.register_lr9e_probe() {
        Ok(task) => task,
        Err(e) => {
            ACTIVE.store(false, Ordering::Release);
            return Err(e);
        }
    };
    LAST_TASK.store(task.0, Ordering::Release);
    let guard = ActiveTask::new(registry.clone(), task);
    registry.mark_running(task);
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let p = PassiveTracePublisher::production();
        let core = TaskTraceAdapter::new(p.clone(), SourceType::Core, "core");
        core.observe(task, &TaskEventKind::TaskStarted);
        TaskTraceAdapter::new(p.clone(), SourceType::TaskGraph, "task_graph")
            .observe(task, &TaskEventKind::TaskPlanned { step_count: 2 });
        let mut registry_p = ProviderRegistry::default();
        for id in ["fixture_p", "fixture_q"] {
            registry_p
                .register(
                    ProviderConfig {
                        id: id.into(),
                        enabled: true,
                        priority: 1,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    Arc::new(FakeProvider(id)),
                )
                .unwrap();
        }
        let scheduler = Arc::new(Scheduler::new(registry_p));
        let mut handles = vec![];
        for n in 0..3 {
            let (scheduler, cancel, p) = (scheduler.clone(), cancel.clone(), p.clone());
            handles.push(tauri::async_runtime::spawn(async move {
                let worker = TaskTraceAdapter::new(p.clone(), SourceType::Worker, "worker");
                if n > 0 {
                    worker.observe(task, &TaskEventKind::TaskStarted);
                }
                let mut trace = SchedulerTraceAdapter::new(
                    p,
                    SchedulerTraceContext::new(
                        Some(task),
                        if n == 0 {
                            None
                        } else if n == 1 {
                            Some("native-worker-a")
                        } else {
                            Some("native-worker-b")
                        },
                        if n == 0 {
                            CognitiveRole::Conversation
                        } else {
                            CognitiveRole::Worker
                        },
                    ),
                );
                let mut sink = |e| {
                    trace.observe(&e);
                    Ok(())
                };
                let result = scheduler
                    .run_with_retry(
                        request(n > 0),
                        TaskBudget {
                            max_provider_calls: 3,
                            max_output_tokens: Some(100),
                        },
                        RetryPolicy {
                            enabled: true,
                            max_retries: 1,
                            initial_backoff_ms: 0,
                        },
                        &cancel,
                        &mut sink,
                    )
                    .await;
                if n > 0 {
                    worker.observe(
                        task,
                        &if cancel.load(Ordering::Acquire) {
                            TaskEventKind::TaskCancelled
                        } else {
                            TaskEventKind::TaskCompleted
                        },
                    );
                }
                result
            }));
        }
        // Passive fake agent observations; protocol failure/turn counts are proved
        // separately by the real Codex operation fakes in the Rust matrix.
        for _ in 0..2 {
            let trace = AgentTraceAdapter::new(
                p.clone(),
                AgentTraceContext {
                    source_id: "codex".into(),
                    task_id: Some(task),
                    subtask_id: None,
                },
            );
            trace.observe(AgentTraceObservation::Lifecycle(&AgentEvent::WorkStarted));
            trace.observe(AgentTraceObservation::AgentMessage("NATIVE-ALLOWED-AGENT"));
            trace.observe(AgentTraceObservation::Lifecycle(&AgentEvent::Completed));
        }
        let mut failed = false;
        for h in handles {
            if !matches!(h.await, Ok(Ok(_))) {
                failed = true;
            }
        }
        let state = registry.finish(
            task,
            if failed {
                TaskState::Failed
            } else {
                TaskState::Completed
            },
        );
        core.observe(
            task,
            &match state {
                TaskState::Cancelled => TaskEventKind::TaskCancelled,
                TaskState::Failed => TaskEventKind::TaskFailed {
                    detail: "fixture failure".into(),
                },
                _ => TaskEventKind::TaskCompleted,
            },
        );
        ACTIVE.store(false, Ordering::Release);
    });
    Ok(())
}
pub(crate) fn action(app: &AppHandle, action: &str) -> Option<Result<(), String>> {
    if !action.starts_with("lr9e_") {
        return None;
    }
    Some(match action {
        "lr9e_workload" => workload(app),
        "lr9e_cancel" => {
            app.state::<Arc<TaskRegistry>>()
                .cancel(TaskId(LAST_TASK.load(Ordering::Acquire)));
            Ok(())
        }
        "lr9e_collapse" | "lr9e_expand" => app
            .get_webview_window("main")
            .ok_or("no main".to_owned())
            .and_then(|w| {
                w.eval("document.querySelector('.terminal-activity-toggle').click()")
                    .map_err(|e| e.to_string())
            }),
        "lr9e_snapshot" => Ok(()),
        "lr9e_pty_marker" => {
            let human = app.state::<Arc<crate::execution::human::HumanTerminal>>();
            human.status().ok_or("no human PTY".to_owned()).and_then(|s| {
                        human.input(&s.session_id, b"printf '%s%s\\n' PTY-ONLY-PRIVATE- MARKER\n").map_err(|e| format!("{e:?}"))
            })
        }
        #[cfg(not(debug_assertions))]
        "lr9e_release_security" => app
            .get_webview_window("main")
            .ok_or("no main".to_owned())
            .and_then(|w| {
                w.eval(r#"(async()=>{const names=['start_mock_task','start_mock_cognition_task','cognition_provider_status','security_test_store_secret','security_test_delete_secret','lr4_status','lr4_import_private_bootstrap','lr4_create_diagnostic_conversation','lr4_get_recent_conversation'];const results=[];for(const command of names){let denied=false,reason='unexpected success';try{await window.__TAURI_INTERNALS__.invoke(command,{})}catch(error){reason=String(error);denied=/not allowed|not found|not permitted/i.test(reason)}results.push({command,denied,reason})}await window.__TAURI_INTERNALS__.invoke('perf1c_ui_report',{report:{gate:'LR9E_RELEASE_IPC',results}})})()"#)
                    .map_err(|e| e.to_string())
            }),
        _ => Err("unknown fixed LR-9E action".into()),
    })
}
pub(crate) fn snapshot(app: &AppHandle) -> Value {
    let r = app.state::<Arc<TaskRegistry>>();
    let s = app.state::<Arc<crate::cognition::summary::SummaryWorker>>();
    let stats = app.state::<Arc<OperationalTraceBus>>().stats();
    let bus = app.state::<Arc<OperationalTraceBus>>();
    let mut after = 0;
    let mut sources = std::collections::BTreeSet::new();
    let mut private = false;
    loop {
        let batch = bus.replay(after, BatchLimits::default()).unwrap();
        after = batch.next_after;
        for e in batch.events {
            sources.insert(format!("{:?}", e.provenance().source.source_type));
            let text = match e.kind() {
                OperationalKind::TextDelta { text, .. } => text.as_str(),
                OperationalKind::State { detail, .. } => detail.as_str(),
                OperationalKind::Critical { message, .. } => message.as_str(),
            };
            private |= [
                "USER-INPUT-PRIVATE-MARKER",
                "WORKER-INTERNAL-PRIVATE-MARKER",
                "SUMMARY-TRANSCRIPT-PRIVATE-MARKER",
                "ENVIRONMENT-PRIVATE-MARKER",
                "PTY-ONLY-PRIVATE-MARKER",
            ]
            .iter()
            .any(|m| text.contains(m));
        }
        if !batch.has_more {
            break;
        }
    }
    json!({"surfaceWorkers":app.state::<crate::terminal_surface::SurfaceHub>().worker_count(),"brokerActive":app.state::<Arc<crate::execution::ExecutionBroker>>().active_count(),"brokerWorkers":app.state::<Arc<crate::execution::ExecutionBroker>>().worker_count(),"workloadActive":ACTIVE.load(Ordering::Acquire),"hygienePrivateMarkerAbsent":!private,"sources":sources,"calls":CALLS.load(Ordering::Acquire),"chunks":CHUNKS.load(Ordering::Acquire),"taskId":LAST_TASK.load(Ordering::Acquire),"taskActive":r.active_count(),"taskWorkers":r.worker_count(),"summaryStopped":s.stopped(),"trace":{"published":stats.published,"retainedEvents":stats.retained_events,"retainedBytes":stats.retained_bytes,"activeSubscribers":stats.active_subscribers,"evicted":{"stream":stats.evicted.stream,"state":stats.evicted.state,"critical":stats.evicted.critical},"dropped":{"stream":stats.dropped.stream,"state":stats.dropped.state,"critical":stats.dropped.critical},"liveDrops":stats.live_delivery_dropped}})
}
