use super::*;
use std::{thread, time::Instant};
fn until(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}
struct Runtime {
    broker: Arc<ExecutionBroker>,
    human: Arc<HumanTerminal>,
    hub: Arc<SurfaceHub>,
}
impl Runtime {
    fn new() -> Self {
        let broker = ExecutionBroker::isolated();
        Self {
            human: Arc::new(HumanTerminal::new(broker.clone())),
            broker,
            hub: Arc::new(SurfaceHub::default()),
        }
    }
    fn shell(&self) -> PtySession {
        let dto = self.human.open().unwrap();
        let s = self.human.find(&dto.session_id).unwrap();
        until(|| s.state() == ExecutionState::Running);
        s
    }
    fn idle(&self) {
        until(|| self.broker.stopped() && self.hub.worker_count() == 0);
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.hub.detach_main();
        self.broker.request_shutdown();
        assert!(self.broker.wait_shutdown(SHUTDOWN_DEADLINE));
        until(|| self.hub.worker_count() == 0);
    }
}
fn tail(s: &PtySession) -> Vec<u8> {
    let mut cursor = 0;
    let mut bytes = Vec::new();
    loop {
        let b = s
            .replay(cursor, MAX_BATCH_CHUNKS, crate::execution::MAX_BATCH_BYTES)
            .unwrap();
        for c in b.chunks {
            bytes.extend_from_slice(&c.bytes);
        }
        cursor = b.next_after;
        if !b.has_more {
            return bytes;
        }
    }
}
fn marker(s: &PtySession, text: &str) {
    until(|| tail(s).windows(text.len()).any(|w| w == text.as_bytes()));
}
fn stream(source: &str, text: &str) -> EventDraft {
    EventDraft::new(
        Provenance {
            source: TraceSource {
                source_type: SourceType::Worker,
                id: TraceId::new(source).unwrap(),
                instance: None,
            },
            task_id: None,
            subtask_id: None,
            correlation_id: None,
            coalescing_key: Some(TraceId::new("delta").unwrap()),
        },
        OperationalKind::TextDelta {
            channel: TextChannel::Stdout,
            text: TraceText::new(text).unwrap(),
        },
    )
    .unwrap()
}
#[test]
fn registry_product_boundary_reattach_same_id_pid_and_native_directory() {
    let rt = Runtime::new();
    let s = rt.shell();
    let first = rt.human.open().unwrap();
    assert_eq!(first.session_id, s.id().get().to_string());
    assert_eq!(
        rt.human.find(&first.session_id).unwrap().process_id(),
        s.process_id()
    );
    assert_eq!(s.origin(), &ExecutionOrigin::Human);
    assert_eq!(
        s.request().cwd,
        crate::execution::human::starting_directory()
    );
    assert!(s.request().args.is_empty());
    assert!(matches!(
        s.request().environment,
        EnvironmentPolicy::HumanInherited
    ));
    assert!(rt.human.find("arbitrary-pid").is_err());
}
#[test]
fn human_interactive_policy_does_not_expire_at_artificial_hour_structured_policy_remains_bounded() {
    use crate::execution::pty::PtyLifecycle;
    let elapsed = MAX_TIMEOUT + Duration::from_secs(1);
    assert!(!PtyLifecycle::HumanInteractive.expired(elapsed, MAX_TIMEOUT));
    assert!(PtyLifecycle::Bounded.expired(elapsed, MAX_TIMEOUT));
    assert!(include_str!("../execution/broker.rs").contains("Instant::now() >= deadline"));
}
#[test]
fn attachment_drop_and_disconnect_preserve_running_pty() {
    let rt = Runtime::new();
    let s = rt.shell();
    let live = s.subscribe().unwrap();
    drop(live);
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let live = s.subscribe().unwrap();
    let aa = a.clone();
    let ss = s.clone();
    rt.hub
        .spawn(a, move || {
            run_pty_bridge(&aa, ss, live, |_| false, |_| true)
        })
        .unwrap();
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(s.state(), ExecutionState::Running);
    assert_eq!(s.subscriber_count(), 0);
    rt.human
        .input(
            &s.id().get().to_string(),
            b"printf '%s%s\\n' AFTER_ DISCONNECT\n",
        )
        .unwrap();
    marker(&s, "AFTER_DISCONNECT");
}
#[test]
fn repeated_attach_detach_bounded_workers_and_subscribers() {
    let rt = Runtime::new();
    let s = rt.shell();
    for _ in 0..20 {
        let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
        let aa = a.clone();
        let live = s.subscribe().unwrap();
        let ss = s.clone();
        rt.hub
            .spawn(a, move || {
                run_pty_bridge(
                    &aa,
                    ss,
                    live,
                    |b| {
                        aa.ack(
                            0,
                            &u64::from_le_bytes(b[..8].try_into().unwrap()).to_string(),
                        )
                        .unwrap();
                        true
                    },
                    |_| true,
                )
            })
            .unwrap();
        rt.hub.detach_main();
        until(|| rt.hub.worker_count() == 0);
        assert_eq!(s.subscriber_count(), 0);
    }
    assert_eq!(s.state(), ExecutionState::Running);
}
#[test]
fn slow_live_subscriber_is_bounded_and_reader_recovers_after_overflow() {
    let rt = Runtime::new();
    let s = rt.shell();
    let slow = s.subscribe().unwrap();
    assert_eq!(
        slow.cursor,
        s.replay(0, 1, READ_CHUNK_BYTES).unwrap().latest_sequence
    );
    rt.human.input(&s.id().get().to_string(), b"stty -echo; /usr/bin/python3 -c \"import os; [os.write(1,b'x'*8192) for _ in range(768)]\"; printf '%s%s\\n' STRESS_ ALIVE\n").unwrap();
    marker(&s, "STRESS_ALIVE");
    let b = s
        .replay(0, MAX_BATCH_CHUNKS, crate::execution::MAX_BATCH_BYTES)
        .unwrap();
    assert!(b.gap && b.dropped_bytes > 2 * 1024 * 1024);
    assert!(b.retained_bytes <= PTY_RETAIN_BYTES && b.retained_chunks <= PTY_RETAIN_CHUNKS);
    assert!(slow.lost_notifications() > 0);
    assert!(slow.wait(Duration::ZERO).is_some());
    assert!(slow.wait(Duration::ZERO).is_none());
    assert_eq!(s.state(), ExecutionState::Running);
    drop(slow);
    // New real bridge receives bounded replay and factual gap; raw bytes exact.
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let aa = a.clone();
    let live = s.subscribe().unwrap();
    let ss = s.clone();
    let frames = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let captured = frames.clone();
    rt.hub
        .spawn(a, move || {
            run_pty_bridge(
                &aa,
                ss,
                live,
                |b| {
                    let cursor = u64::from_le_bytes(b[..8].try_into().unwrap()).to_string();
                    captured.lock().unwrap().push(b);
                    aa.ack(0, &cursor).unwrap();
                    true
                },
                |_| true,
            )
        })
        .unwrap();
    until(|| {
        frames
            .lock()
            .unwrap()
            .iter()
            .any(|b| b.windows(13).any(|w| w == b"STRESS_ALIVE\r"))
    });
    assert!(u64::from_le_bytes(frames.lock().unwrap()[0][8..16].try_into().unwrap()) > 0);
    rt.human.resize(&s.id().get().to_string(), 39, 111).unwrap();
    rt.human
        .input(
            &s.id().get().to_string(),
            b"stty size; printf '%s%s\\n' INPUT_ AFTER\n",
        )
        .unwrap();
    marker(&s, "39 111");
    marker(&s, "INPUT_AFTER");
    rt.hub.detach_main();
    rt.human
        .input(&s.id().get().to_string(), b"exit\n")
        .unwrap();
    assert!(s.wait(Duration::from_secs(10)).unwrap().reaped);
    rt.idle();
    assert_eq!(s.subscriber_count(), 0);
    eprintln!(
        "LR-9C PTY stress: total={} dropped={} retained={} workers=0",
        b.total_bytes, b.dropped_bytes, b.retained_bytes
    );
}
#[test]
fn close_prunes_completed_handle_and_quit_reaps_product_session() {
    let rt = Runtime::new();
    let s = rt.shell();
    let id = s.id().get().to_string();
    assert!(rt.human.close(&id).unwrap());
    let result = s.wait(Duration::from_secs(10)).unwrap();
    assert!(result.reaped);
    assert_eq!(rt.human.status().unwrap().state, "cancelled");
    let next = rt.shell();
    assert_ne!(next.id(), s.id());
    assert!(rt.human.find(&id).is_err());
    let pid = next.process_id().unwrap();
    rt.broker.request_shutdown();
    rt.idle();
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}
#[test]
fn attachment_acks_scoped_ordered_and_detach_revokes_input() {
    let hub = SurfaceHub::default();
    let a = hub.create(Some("9007199254740993".into())).unwrap();
    assert_eq!(
        session_for_attachment(&hub, &a.id).unwrap(),
        "9007199254740993"
    );
    assert!(a.prepare(0, "9007199254740993".into()));
    assert!(!a.prepare(0, "other".into()));
    assert!(a.ack(0, "9007199254740992").is_err());
    assert!(a.ack(1, "9007199254740993").is_err());
    assert!(a.ack(0, "9007199254740993").is_ok());
    hub.detach_main();
    assert!(session_for_attachment(&hub, &a.id).is_err());
}
#[test]
fn stalled_attachment_does_not_accumulate_channel_batches() {
    let rt = Runtime::new();
    let s = rt.shell();
    let a = rt.hub.create(Some(s.id().get().to_string())).unwrap();
    let aa = a.clone();
    let ss = s.clone();
    let live = s.subscribe().unwrap();
    let sends = Arc::new(AtomicUsize::new(0));
    let count = sends.clone();
    rt.hub
        .spawn(a.clone(), move || {
            run_pty_bridge(
                &aa,
                ss,
                live,
                |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    true
                },
                |_| true,
            )
        })
        .unwrap();
    until(|| sends.load(Ordering::Relaxed) == 1);
    rt.human
        .input(
            &s.id().get().to_string(),
            b"printf '%s%s\\n' CORE_ PROGRESS\n",
        )
        .unwrap();
    marker(&s, "CORE_PROGRESS");
    assert_eq!(sends.load(Ordering::Relaxed), 1);
    until(|| a.stopped());
    until(|| rt.hub.worker_count() == 0);
    assert_eq!(s.state(), ExecutionState::Running);
}
#[test]
fn trace_dto_exact_large_identity_and_no_request_environment() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("a", "á\n")).unwrap();
    bus.publish(stream("a", "🦀  ")).unwrap();
    let mut dto = trace_batch(0, bus.replay(0, BatchLimits::default()).unwrap(), 0);
    assert_eq!(dto.events.len(), 1);
    assert_eq!(dto.events[0].text, "á\n🦀  ");
    assert_eq!(dto.events[0].last_sequence, "2");
    dto.events[0].sequence = u64::MAX.to_string();
    dto.cursor = "9007199254740993".into();
    let json = serde_json::to_value(dto).unwrap();
    assert_eq!(json["events"][0]["sequence"], "18446744073709551615");
    assert_eq!(json["cursor"], "9007199254740993");
    for absent in ["environment", "request", "args", "program", "authority"] {
        assert!(!json.to_string().contains(absent));
    }
}
#[test]
fn trace_bridge_replay_live_gap_reattach_and_dead_channel_publishers_progress() {
    let bus = OperationalTraceBus::isolated();
    bus.publish(stream("a", "initial ")).unwrap();
    let hub = SurfaceHub::default();
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let bb = bus.clone();
    let live = bus.subscribe().unwrap();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let found = batches.clone();
    hub.spawn(a, move || {
        run_trace_bridge(&aa, bb, live, |b| {
            aa.ack(1, &b.cursor).unwrap();
            found.lock().unwrap().push(b);
            true
        })
    })
    .unwrap();
    until(|| !batches.lock().unwrap().is_empty());
    bus.publish(stream("b", "live")).unwrap();
    until(|| {
        batches
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| &b.events)
            .any(|e| e.text == "live")
    });
    hub.detach_main();
    until(|| hub.worker_count() == 0);
    assert_eq!(bus.stats().active_subscribers, 0);
    for i in 0..5000 {
        bus.publish(stream(if i % 2 == 0 { "a" } else { "b" }, "flood"))
            .unwrap();
    }
    let replay = trace_batch(0, bus.replay(0, BatchLimits::default()).unwrap(), 0);
    assert!(!replay.replay_complete);
    assert!(replay.missing_events.parse::<u64>().unwrap() > 0);
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let live = bus.subscribe().unwrap();
    let bb = bus.clone();
    hub.spawn(a, move || run_trace_bridge(&aa, bb, live, |_| false))
        .unwrap();
    until(|| hub.worker_count() == 0);
    assert!(bus.publish(stream("a", "after death")).is_ok());
    assert_eq!(bus.stats().active_subscribers, 0);
}
#[test]
fn trace_slow_attachment_multi_source_stress_preserves_state_critical_without_webview_ack() {
    let bus = OperationalTraceBus::isolated();
    let hub = SurfaceHub::default();
    let a = hub.create(None).unwrap();
    let aa = a.clone();
    let bb = bus.clone();
    let live = bus.subscribe().unwrap();
    let sent = Arc::new(AtomicUsize::new(0));
    let count = sent.clone();
    hub.spawn(a, move || {
        run_trace_bridge(&aa, bb, live, |_| {
            count.fetch_add(1, Ordering::Relaxed);
            true
        })
    })
    .unwrap();
    until(|| sent.load(Ordering::Relaxed) == 1);
    for i in 0..6000 {
        let source = format!("source-{}", i % 4);
        let d = stream(&source, "á exact delta\n");
        let p = d.clone();
        bus.publish(p).unwrap();
        if i % 100 == 0 {
            let mut provenance = Provenance {
                source: TraceSource {
                    source_type: SourceType::Core,
                    id: TraceId::new("fixture").unwrap(),
                    instance: None,
                },
                task_id: None,
                subtask_id: None,
                correlation_id: None,
                coalescing_key: None,
            };
            provenance.task_id = Some(crate::luna::task::TaskId(i + 1));
            bus.publish(
                EventDraft::new(
                    provenance.clone(),
                    OperationalKind::State {
                        kind: StateKind::Checkpoint,
                        code: TraceId::new("fixed").unwrap(),
                        detail: TraceText::new("state").unwrap(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
            bus.publish(
                EventDraft::new(
                    provenance,
                    OperationalKind::Critical {
                        kind: CriticalKind::Completed,
                        code: TraceId::new("fixed").unwrap(),
                        message: TraceText::new("critical").unwrap(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    let stats = bus.stats();
    assert_eq!(stats.published, 6120);
    assert!(stats.live_delivery_dropped > 0);
    assert_eq!(stats.evicted.state, 0);
    assert_eq!(stats.evicted.critical, 0);
    assert_eq!(sent.load(Ordering::Relaxed), 1);
    hub.detach_main();
    until(|| hub.worker_count() == 0);
    let mut cursor = 0;
    let mut state = 0;
    let mut critical = 0;
    loop {
        let b = bus.replay(cursor, BatchLimits::default()).unwrap();
        state += b
            .events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::State)
            .count();
        critical += b
            .events
            .iter()
            .filter(|e| e.retention_class() == RetentionClass::Critical)
            .count();
        cursor = b.next_after;
        if !b.has_more {
            break;
        }
    }
    assert_eq!((state, critical), (60, 60));
    assert_eq!(bus.stats().active_subscribers, 0);
    eprintln!("LR-9C trace stress: published={} retained={} live_dropped={} state={} critical={} in_flight=1",stats.published,stats.retained_events,stats.live_delivery_dropped,state,critical);
}
#[test]
fn security_boundary_no_generic_executor_provider_or_agent_path() {
    let source = include_str!("mod.rs");
    for forbidden in [
        "execute_command",
        "ExecutionAuthority",
        "crate::agents",
        "crate::cognition",
        "ExecutionRequest",
        "EnvironmentPolicy",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/main-window.json")).unwrap();
    assert_eq!(main["windows"], serde_json::json!(["main"]));
    for settings in [
        include_str!("../../capabilities/settings-general.json"),
        include_str!("../../capabilities/settings-ai.json"),
    ] {
        for forbidden in [
            "human-terminal",
            "terminal-input",
            "terminal-surface",
            "resize-terminal",
        ] {
            assert!(!settings.contains(forbidden));
        }
    }
}

#[test]
fn actual_session_and_bus_ids_cross_serialization_above_js_safe_integer() {
    let rt = Runtime::new();
    rt.broker.seed_test_id(9_007_199_254_740_992);
    let s = rt.shell();
    let dto = serde_json::to_value(rt.human.status().unwrap()).unwrap();
    assert_eq!(dto["sessionId"], "9007199254740993");
    assert_eq!(s.id().get(), 9_007_199_254_740_993);
    let bus = OperationalTraceBus::isolated();
    bus.seed_test_sequence(9_007_199_254_740_992);
    bus.publish(stream("exact", "native fact")).unwrap();
    let projected = trace_batch(
        9_007_199_254_740_992,
        bus.replay(9_007_199_254_740_992, BatchLimits::default())
            .unwrap(),
        0,
    );
    let json = serde_json::to_value(projected).unwrap();
    assert_eq!(json["events"][0]["sequence"], "9007199254740993");
    assert_eq!(json["cursor"], "9007199254740993");
    let raw = pty_frame(0, &s.replay(0, 1, READ_CHUNK_BYTES).unwrap());
    assert!(raw.len() >= 24);
}

#[test]
fn simultaneous_worker_admission_never_exceeds_bridge_budget() {
    let hub = Arc::new(SurfaceHub::default());
    let a = Arc::new(Attachment::new("fixture".into(), None));
    let start = Arc::new(std::sync::Barrier::new(16));
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let hub = hub.clone();
            let a = a.clone();
            let start = start.clone();
            let release = release.clone();
            thread::spawn(move || {
                start.wait();
                hub.spawn(a, move || {
                    let (lock, wake) = &*release;
                    let _guard = wake
                        .wait_while(lock.lock().unwrap(), |released| !*released)
                        .unwrap();
                })
                .is_ok()
            })
        })
        .collect();
    let admitted = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(admitted, MAX_BRIDGE_WORKERS);
    assert!(hub.worker_count() <= MAX_BRIDGE_WORKERS);
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    until(|| hub.worker_count() == 0);
}

#[cfg(target_os = "linux")]
#[test]
fn activity_fixture_uses_real_adapters_and_existing_lr9c_dto() {
    use crate::agents::{
        trace::{AgentTraceObservation, AgentTraceSink},
        types::AgentEvent,
    };
    use crate::cognition::{policy::CognitiveRole, scheduler::SchedulerEvent};
    use crate::luna::task::{TaskEventKind, TaskId};
    use crate::operational_trace::adapters::tests::{events, selected, subtask_started};
    use crate::operational_trace::adapters::*;
    let b = OperationalTraceBus::isolated();
    let p = PassiveTracePublisher::new(b.clone());
    TaskTraceAdapter::new(p.clone(), SourceType::Core, "core")
        .observe(TaskId(17), &TaskEventKind::TaskStarted);
    let mut s = SchedulerTraceAdapter::new(
        p.clone(),
        SchedulerTraceContext::new(Some(TaskId(17)), None, CognitiveRole::Conversation),
    );
    s.observe(&selected(1));
    s.observe(&SchedulerEvent::Chunk {
        provider_id: "p".into(),
        text: "Saída pública 🦀".into(),
    });
    TaskTraceAdapter::new(p.clone(), SourceType::TaskGraph, "task_graph")
        .observe(TaskId(17), &TaskEventKind::TaskPlanned { step_count: 2 });
    TaskTraceAdapter::new(p.clone(), SourceType::Worker, "worker")
        .observe(TaskId(17), &subtask_started());
    let a = AgentTraceAdapter::new(
        p,
        AgentTraceContext {
            source_id: "codex".into(),
            task_id: Some(TaskId(17)),
            subtask_id: None,
        },
    );
    a.observe(AgentTraceObservation::Lifecycle(&AgentEvent::SessionReady));
    a.observe(AgentTraceObservation::AgentMessage(
        "Mensagem natural do planner",
    ));
    a.observe(AgentTraceObservation::DisplayReasoningSummary(
        "Resumo explicitamente exibível",
    ));
    a.observe(AgentTraceObservation::Lifecycle(&AgentEvent::Completed));
    let dto: Vec<_> = events(&b)
        .iter()
        .map(|e| crate::terminal_surface::dto::TraceDto::event(e))
        .collect();
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(dto.len(), 9);
    if let Ok(path) = std::env::var("NARYS_LR9D_ACTIVITY_FIXTURE") {
        std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    }
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceType"] == "specialist_agent" && e["sourceId"] == "codex"));
}
